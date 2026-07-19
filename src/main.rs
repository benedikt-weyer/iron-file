use std::{
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread,
    time::Duration,
};

use iced::{
    Element, Length, Subscription, Task,
    widget::{button, column, container, text},
};

/// Events sent by the GUI thread to the main thread.
#[derive(Debug)]
enum GuiEvent {
    ButtonPressed,
}

/// Updates sent by the main thread back to the GUI thread.
#[derive(Debug)]
enum MainEvent {
    Status(String),
}

fn main() {
    prefer_x11_when_available();

    let (gui_sender, gui_receiver) = mpsc::channel::<GuiEvent>();
    let (main_sender, main_receiver) = mpsc::channel::<MainEvent>();

    let worker_thread = thread::spawn(move || {
        run_background_worker(gui_receiver, main_sender);
    });

    // Window event loops must run on the process main thread for portability.
    iced::application("Iron File", Gui::update, Gui::view)
        .subscription(Gui::subscription)
        .run_with(|| (Gui::new(gui_sender, main_receiver), Task::none()))
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

fn run_background_worker(gui_receiver: Receiver<GuiEvent>, main_sender: Sender<MainEvent>) {
    // The worker owns application work and communicates only through channels.
    while let Ok(GuiEvent::ButtonPressed) = gui_receiver.recv() {
        let status = format!(
            "Background worker received the button press at {:?}",
            std::time::SystemTime::now()
        );

        if main_sender.send(MainEvent::Status(status)).is_err() {
            break;
        }
    }
}

struct Gui {
    gui_sender: Sender<GuiEvent>,
    main_receiver: Receiver<MainEvent>,
    status: String,
}

#[derive(Debug, Clone)]
enum Message {
    PressButton,
    PollMainThread,
}

impl Gui {
    fn new(gui_sender: Sender<GuiEvent>, main_receiver: Receiver<MainEvent>) -> Self {
        Self {
            gui_sender,
            main_receiver,
            status: "Waiting for input".into(),
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PressButton => {
                if self.gui_sender.send(GuiEvent::ButtonPressed).is_ok() {
                    self.status = "Sent button press to the main thread".into();
                } else {
                    self.status = "Main thread is no longer available".into();
                }
            }
            Message::PollMainThread => loop {
                match self.main_receiver.try_recv() {
                    Ok(MainEvent::Status(status)) => self.status = status,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.status = "Main thread disconnected".into();
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
            text("GUI and main thread communication").size(24),
            button("Send message to main thread").on_press(Message::PressButton),
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
