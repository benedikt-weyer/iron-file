use std::{
    path::PathBuf,
    sync::mpsc::{Receiver, Sender, TryRecvError},
    time::Duration,
};

use iced::{
    Element, Length, Subscription, Task,
    widget::{button, column, container, row, scrollable, text, text_input},
};
use iron_file::{BackendEvent, FileEntry, GuiEvent, start_backend};

fn main() {
    prefer_x11_when_available();

    let (gui_sender, backend_receiver, worker_thread) = start_backend();
    let initial_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let _ = gui_sender.send(GuiEvent::OpenPath(initial_path));

    iced::application("Iron File", Gui::update, Gui::view)
        .subscription(Gui::subscription)
        .run_with(|| (Gui::new(gui_sender, backend_receiver), Task::none()))
        .expect("failed to start GUI");

    worker_thread.join().expect("background worker panicked");
}

#[cfg(target_os = "linux")]
fn prefer_x11_when_available() {
    if std::env::var_os("DISPLAY").is_some() {
        // winit otherwise prefers WAYLAND_DISPLAY, even when its client library is unavailable.
        unsafe {
            std::env::remove_var("WAYLAND_DISPLAY");
            std::env::remove_var("WAYLAND_SOCKET");
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn prefer_x11_when_available() {}

struct Gui {
    gui_sender: Sender<GuiEvent>,
    backend_receiver: Receiver<BackendEvent>,
    directory_path: PathBuf,
    address: String,
    entries: Vec<FileEntry>,
    content: String,
    status: String,
}

#[derive(Debug, Clone)]
enum Message {
    AddressChanged(String),
    OpenAddress,
    OpenPath(PathBuf),
    OpenParent,
    PollBackend,
}

impl Gui {
    fn new(gui_sender: Sender<GuiEvent>, backend_receiver: Receiver<BackendEvent>) -> Self {
        let directory_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        Self {
            gui_sender,
            backend_receiver,
            address: directory_path.display().to_string(),
            directory_path,
            entries: Vec::new(),
            content: String::new(),
            status: "Loading directory".into(),
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::AddressChanged(address) => self.address = address,
            Message::OpenAddress => self.open_path(PathBuf::from(&self.address)),
            Message::OpenPath(path) => self.open_path(path),
            Message::OpenParent => {
                if let Some(parent) = self.directory_path.parent() {
                    self.open_path(parent.to_path_buf());
                }
            }
            Message::PollBackend => loop {
                match self.backend_receiver.try_recv() {
                    Ok(BackendEvent::Directory { path, entries }) => {
                        self.address = path.display().to_string();
                        self.directory_path = path;
                        self.entries = entries;
                        self.content.clear();
                        self.status = format!("{} entries", self.entries.len());
                    }
                    Ok(BackendEvent::FileContent { path, content }) => {
                        self.address = path.display().to_string();
                        self.content = content;
                        self.status = "File preview".into();
                    }
                    Ok(BackendEvent::Error { path, message }) => {
                        self.status = format!("{}: {message}", path.display());
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.status = "Background worker disconnected".into();
                        break;
                    }
                }
            },
        }

        Task::none()
    }

    fn open_path(&mut self, path: PathBuf) {
        if self.gui_sender.send(GuiEvent::OpenPath(path)).is_ok() {
            self.status = "Loading".into();
        } else {
            self.status = "Background worker is no longer available".into();
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        iced::time::every(Duration::from_millis(50)).map(|_| Message::PollBackend)
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
                    .on_press(Message::OpenPath(entry.path.clone())),
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

        let content = column![address_bar, text(&self.status), browser]
            .spacing(12)
            .padding(16)
            .height(Length::Fill);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
