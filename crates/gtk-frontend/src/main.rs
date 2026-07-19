use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread,
    time::Duration,
};

use gtk4::{
    Application, ApplicationWindow, Button, Entry, Label, ListBox, Orientation, ScrolledWindow,
    TextView,
    glib::{self, ControlFlow},
    prelude::*,
};
use hyper_util::rt::TokioIo;
use tokio::{net::UnixStream, runtime::Runtime};
use tonic::{
    Request,
    transport::{Endpoint, Uri},
};
use tower::service_fn;

pub mod proto {
    tonic::include_proto!("ironfile.v1");
}

use proto::{
    BrowseResponse, OpenPathRequest, browse_response::Payload,
    file_browser_client::FileBrowserClient,
};

fn main() {
    let (response_sender, response_receiver) = mpsc::channel();
    let response_receiver = Rc::new(RefCell::new(Some(response_receiver)));
    let app = Application::builder()
        .application_id("com.example.iron-file")
        .build();

    app.connect_activate(move |app| {
        if let Some(receiver) = response_receiver.borrow_mut().take() {
            build_ui(app, response_sender.clone(), receiver);
        }
    });
    app.run();
}

fn build_ui(
    app: &Application,
    response_sender: Sender<Result<BrowseResponse, String>>,
    response_receiver: Receiver<Result<BrowseResponse, String>>,
) {
    let status = Label::new(Some("Connecting to backend"));
    let address = Entry::new();
    let initial_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    address.set_text(&initial_path.display().to_string());
    let open_button = Button::with_label("Open");
    let up_button = Button::with_label("Up");
    let file_list = ListBox::new();
    let content_view = TextView::new();
    content_view.set_editable(false);
    content_view.set_monospace(true);

    let address_sender = response_sender.clone();
    let address_entry = address.clone();
    let address_status = status.clone();
    address.connect_activate(move |_| {
        request_address(&address_sender, &address_entry, &address_status);
    });
    let open_sender = response_sender.clone();
    let open_entry = address.clone();
    let open_status = status.clone();
    open_button.connect_clicked(move |_| {
        request_address(&open_sender, &open_entry, &open_status);
    });
    let up_sender = response_sender.clone();
    let up_entry = address.clone();
    let up_status = status.clone();
    up_button.connect_clicked(move |_| {
        let current = PathBuf::from(up_entry.text().as_str());
        if let Some(parent) = current.parent() {
            request_path(&up_sender, parent.to_path_buf());
            up_status.set_text("Loading");
        }
    });

    let worker_status = status.clone();
    let worker_address = address.clone();
    let worker_list = file_list.clone();
    let worker_content = content_view.buffer();
    let worker_sender = response_sender.clone();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        loop {
            match response_receiver.try_recv() {
                Ok(result) => apply_response(
                    result,
                    &worker_address,
                    &worker_list,
                    &worker_content,
                    &worker_status,
                    &worker_sender,
                ),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return ControlFlow::Break,
            }
        }
        ControlFlow::Continue
    });

    let address_bar = gtk4::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .build();
    address_bar.append(&up_button);
    address_bar.append(&address);
    address_bar.append(&open_button);
    let files = ScrolledWindow::builder()
        .child(&file_list)
        .hexpand(true)
        .vexpand(true)
        .build();
    let preview = ScrolledWindow::builder()
        .child(&content_view)
        .hexpand(true)
        .vexpand(true)
        .build();
    let panes = gtk4::Paned::builder()
        .orientation(Orientation::Horizontal)
        .start_child(&files)
        .end_child(&preview)
        .build();
    let content = gtk4::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();
    content.append(&address_bar);
    content.append(&status);
    content.append(&panes);
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Iron File")
        .default_width(900)
        .default_height(600)
        .child(&content)
        .build();
    window.present();

    request_path(&response_sender, initial_path);
}

fn request_address(
    sender: &Sender<Result<BrowseResponse, String>>,
    address: &Entry,
    status: &Label,
) {
    request_path(sender, PathBuf::from(address.text().as_str()));
    status.set_text("Loading");
}

fn request_path(sender: &Sender<Result<BrowseResponse, String>>, path: PathBuf) {
    let sender = sender.clone();
    thread::spawn(move || {
        let result = Runtime::new()
            .map_err(|error| error.to_string())
            .and_then(|runtime| runtime.block_on(browse(path)));
        let _ = sender.send(result);
    });
}

fn apply_response(
    result: Result<BrowseResponse, String>,
    address: &Entry,
    list: &ListBox,
    content: &gtk4::TextBuffer,
    status: &Label,
    sender: &Sender<Result<BrowseResponse, String>>,
) {
    let response = match result {
        Ok(response) => response,
        Err(error) => {
            status.set_text(&error);
            return;
        }
    };
    match response.payload {
        Some(Payload::Directory(directory)) => {
            address.set_text(&response.path);
            replace_entries(list, sender, directory.entries);
            content.set_text("");
            status.set_text("Directory loaded");
        }
        Some(Payload::File(file)) => {
            address.set_text(&response.path);
            content.set_text(&file.content);
            status.set_text("File preview");
        }
        Some(Payload::Error(error)) => status.set_text(&error.message),
        None => status.set_text("Backend returned an invalid response"),
    }
}

fn replace_entries(
    list: &ListBox,
    sender: &Sender<Result<BrowseResponse, String>>,
    entries: Vec<proto::FileEntry>,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    for entry in entries {
        let prefix = if entry.is_directory {
            "[Folder] "
        } else {
            "[File] "
        };
        let button = Button::with_label(&format!("{prefix}{}", entry.name));
        let entry_sender = sender.clone();
        button.connect_clicked(move |_| request_path(&entry_sender, PathBuf::from(&entry.path)));
        list.append(&button);
    }
}

async fn browse(path: PathBuf) -> Result<BrowseResponse, String> {
    let socket = socket_path();
    let endpoint = Endpoint::try_from("http://[::]:50051").map_err(|error| error.to_string())?;
    let channel = endpoint
        .connect_with_connector(service_fn(move |_: Uri| {
            let socket = socket.clone();
            async move { UnixStream::connect(socket).await.map(TokioIo::new) }
        }))
        .await
        .map_err(|error| format!("Cannot connect to the backend: {error}"))?;
    let mut client = FileBrowserClient::new(channel);
    client
        .open_path(Request::new(OpenPathRequest {
            path: path.display().to_string(),
        }))
        .await
        .map(|response| response.into_inner())
        .map_err(|error| error.to_string())
}

fn socket_path() -> PathBuf {
    std::env::var_os("IRON_FILE_SOCKET")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_RUNTIME_DIR")
                .map(|dir| PathBuf::from(dir).join("iron-file-backend.sock"))
        })
        .unwrap_or_else(|| std::env::temp_dir().join("iron-file-backend.sock"))
}
