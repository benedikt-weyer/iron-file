use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::mpsc::{Receiver, Sender, TryRecvError},
    time::Duration,
};

use gtk4::{
    Application, ApplicationWindow, Button, Entry, Label, ListBox, Orientation, ScrolledWindow,
    TextView,
    glib::{self, ControlFlow},
    prelude::*,
};
use iron_file::{BackendEvent, FileEntry, GuiEvent, start_backend};

fn main() {
    let (gui_sender, backend_receiver, worker_thread) = start_backend();
    let initial_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let _ = gui_sender.send(GuiEvent::OpenPath(initial_path));
    let backend_receiver = Rc::new(RefCell::new(Some(backend_receiver)));

    let app = Application::builder()
        .application_id("com.example.iron-file")
        .build();
    let activation_sender = gui_sender.clone();

    app.connect_activate(move |app| {
        if let Some(receiver) = backend_receiver.borrow_mut().take() {
            build_ui(app, activation_sender.clone(), receiver);
        }
    });

    app.run();
    drop(app);
    drop(gui_sender);
    worker_thread.join().expect("background worker panicked");
}

fn build_ui(
    app: &Application,
    gui_sender: Sender<GuiEvent>,
    backend_receiver: Receiver<BackendEvent>,
) {
    let status = Label::new(Some("Loading directory"));
    let address = Entry::new();
    let initial_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    address.set_text(&initial_path.display().to_string());
    let open_button = Button::with_label("Open");
    let up_button = Button::with_label("Up");
    let file_list = ListBox::new();
    let content_view = TextView::new();
    content_view.set_editable(false);
    content_view.set_monospace(true);

    let address_sender = gui_sender.clone();
    let address_entry = address.clone();
    let address_status = status.clone();
    address.connect_activate(move |_| {
        open_address(&address_sender, &address_entry, &address_status);
    });
    let open_sender = gui_sender.clone();
    let open_entry = address.clone();
    let open_status = status.clone();
    open_button.connect_clicked(move |_| {
        open_address(&open_sender, &open_entry, &open_status);
    });
    let up_sender = gui_sender.clone();
    let up_entry = address.clone();
    let up_status = status.clone();
    up_button.connect_clicked(move |_| {
        let current = PathBuf::from(up_entry.text().as_str());
        if let Some(parent) = current.parent() {
            if up_sender
                .send(GuiEvent::OpenPath(parent.to_path_buf()))
                .is_ok()
            {
                up_status.set_text("Loading");
            }
        }
    });

    let worker_status = status.clone();
    let worker_address = address.clone();
    let worker_list = file_list.clone();
    let worker_content = content_view.buffer();
    let worker_sender = gui_sender.clone();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        loop {
            match backend_receiver.try_recv() {
                Ok(BackendEvent::Directory { path, entries }) => {
                    worker_address.set_text(&path.display().to_string());
                    replace_entries(&worker_list, &worker_sender, &worker_status, entries);
                    worker_content.set_text("");
                    worker_status.set_text("Directory loaded");
                }
                Ok(BackendEvent::FileContent { path, content }) => {
                    worker_address.set_text(&path.display().to_string());
                    worker_content.set_text(&content);
                    worker_status.set_text("File preview");
                }
                Ok(BackendEvent::Error { path, message }) => {
                    worker_status.set_text(&format!("{}: {message}", path.display()));
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    worker_status.set_text("Background worker disconnected");
                    return ControlFlow::Break;
                }
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
}

fn open_address(sender: &Sender<GuiEvent>, address: &Entry, status: &Label) {
    if sender
        .send(GuiEvent::OpenPath(PathBuf::from(address.text().as_str())))
        .is_ok()
    {
        status.set_text("Loading");
    } else {
        status.set_text("Background worker is no longer available");
    }
}

fn replace_entries(
    list: &ListBox,
    sender: &Sender<GuiEvent>,
    status: &Label,
    entries: Vec<FileEntry>,
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
        let entry_status = status.clone();
        button.connect_clicked(move |_| {
            if entry_sender
                .send(GuiEvent::OpenPath(entry.path.clone()))
                .is_ok()
            {
                entry_status.set_text("Loading");
            } else {
                entry_status.set_text("Background worker is no longer available");
            }
        });
        list.append(&button);
    }
}
