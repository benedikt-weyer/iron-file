use std::path::PathBuf;

use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, scrollable, text, text_input},
};
use iron_file_common::{browse, ensure_backend, proto};
use proto::{BrowseResponse, browse_response::Payload};
use tokio::runtime::Runtime;

fn main() -> iced::Result {
    prefer_x11_when_available();
    if let Ok(runtime) = Runtime::new() {
        let _ = runtime.block_on(ensure_backend());
    }
    iced::application("Iron File", Gui::update, Gui::view)
        .run_with(|| (Gui::new(), Gui::load_initial_directory()))
}

#[cfg(target_os = "linux")]
fn prefer_x11_when_available() {
    if std::env::var_os("DISPLAY").is_some() {
        unsafe {
            std::env::remove_var("WAYLAND_DISPLAY");
            std::env::remove_var("WAYLAND_SOCKET");
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn prefer_x11_when_available() {}

struct Gui {
    directory_path: PathBuf,
    address: String,
    entries: Vec<proto::FileEntry>,
    content: String,
    status: String,
}

#[derive(Debug, Clone)]
enum Message {
    AddressChanged(String),
    OpenAddress,
    OpenPath(PathBuf),
    OpenParent,
    BrowseFinished(Result<BrowseResponse, String>),
}

impl Gui {
    fn new() -> Self {
        let directory_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            address: directory_path.display().to_string(),
            directory_path,
            entries: Vec::new(),
            content: String::new(),
            status: "Connecting to backend".into(),
        }
    }

    fn load_initial_directory() -> Task<Message> {
        let path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Task::perform(browse(path), Message::BrowseFinished)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::AddressChanged(address) => {
                self.address = address;
                Task::none()
            }
            Message::OpenAddress => self.open_path(PathBuf::from(&self.address)),
            Message::OpenPath(path) => self.open_path(path),
            Message::OpenParent => {
                let parent = self.directory_path.parent().map(|path| path.to_path_buf());
                parent
                    .map(|path| self.open_path(path))
                    .unwrap_or_else(Task::none)
            }
            Message::BrowseFinished(result) => {
                self.apply_response(result);
                Task::none()
            }
        }
    }

    fn open_path(&mut self, path: PathBuf) -> Task<Message> {
        self.status = format!("Loading {}", path.display());
        Task::perform(browse(path), Message::BrowseFinished)
    }

    fn apply_response(&mut self, result: Result<BrowseResponse, String>) {
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                self.status = error;
                return;
            }
        };

        match response.payload {
            Some(Payload::Directory(directory)) => {
                self.address = response.path.clone();
                self.directory_path = PathBuf::from(response.path);
                self.entries = directory.entries;
                self.content.clear();
                self.status = format!("{} entries", self.entries.len());
            }
            Some(Payload::File(file)) => {
                self.address = response.path;
                self.content = file.content;
                self.status = "File preview".into();
            }
            Some(Payload::Error(error)) => self.status = error.message,
            None => self.status = "Backend returned an invalid response".into(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let entries = self.entries.iter().fold(column![], |column, entry| {
            let prefix = if entry.is_directory {
                "[Folder] "
            } else {
                "[File] "
            };
            column.push(
                button(text(format!("{prefix}{}", entry.name)))
                    .width(Length::Fill)
                    .on_press(Message::OpenPath(PathBuf::from(&entry.path))),
            )
        });

        let address_bar = row![
            button("Up").on_press(Message::OpenParent),
            text_input("Path", &self.address)
                .on_input(Message::AddressChanged)
                .on_submit(Message::OpenAddress)
                .width(Length::Fill),
            button("Open").on_press(Message::OpenAddress),
        ]
        .spacing(8);
        let browser = row![
            scrollable(entries).width(Length::FillPortion(1)),
            scrollable(text(&self.content)).width(Length::FillPortion(2)),
        ]
        .spacing(16)
        .height(Length::Fill);

        container(
            column![address_bar, text(&self.status), browser]
                .spacing(12)
                .padding(16)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
