use std::{
    sync::mpsc::{Receiver, Sender, TryRecvError},
    time::Duration,
};

use iced::{
    Element, Length, Subscription, Task,
    widget::{button, column, container, text},
};
use iron_file::{BackendEvent, GuiEvent, start_backend};

fn main() {
    prefer_x11_when_available();

    let (gui_sender, backend_receiver, worker_thread) = start_backend();

    // Window event loops must run on the process main thread for portability.
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
    status: String,
}

#[derive(Debug, Clone)]
enum Message {
    PressButton,
    PollMainThread,
}

impl Gui {
    fn new(gui_sender: Sender<GuiEvent>, backend_receiver: Receiver<BackendEvent>) -> Self {
        Self {
            gui_sender,
            backend_receiver,
            status: "Waiting for input".into(),
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PressButton => {
                if self.gui_sender.send(GuiEvent::ButtonPressed).is_ok() {
                    self.status = "Sent button press to the background worker".into();
                } else {
                    self.status = "Background worker is no longer available".into();
                }
            }
            Message::PollMainThread => loop {
                match self.backend_receiver.try_recv() {
                    Ok(BackendEvent::Status(status)) => self.status = status,
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

    fn subscription(&self) -> Subscription<Message> {
        iced::time::every(Duration::from_millis(50)).map(|_| Message::PollMainThread)
    }

    fn view(&self) -> Element<'_, Message> {
        let content = column![
            text("GUI and background worker communication").size(24),
            button("Send message to background worker").on_press(Message::PressButton),
            text(&self.status),
        ]
        .spacing(16)
        .padding(24);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }
}
