use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
};

/// Events sent from a GUI implementation to the shared background worker.
#[derive(Debug)]
pub enum GuiEvent {
    ButtonPressed,
}

/// Updates sent from the shared background worker to a GUI implementation.
#[derive(Debug)]
pub enum BackendEvent {
    Status(String),
}

/// Starts the backend used by every GUI implementation.
pub fn start_backend() -> (Sender<GuiEvent>, Receiver<BackendEvent>, JoinHandle<()>) {
    let (gui_sender, gui_receiver) = mpsc::channel::<GuiEvent>();
    let (backend_sender, backend_receiver) = mpsc::channel::<BackendEvent>();

    let worker = thread::spawn(move || run_background_worker(gui_receiver, backend_sender));

    (gui_sender, backend_receiver, worker)
}

fn run_background_worker(gui_receiver: Receiver<GuiEvent>, backend_sender: Sender<BackendEvent>) {
    while let Ok(GuiEvent::ButtonPressed) = gui_receiver.recv() {
        let status = format!(
            "Background worker received the button press at {:?}",
            std::time::SystemTime::now()
        );

        if backend_sender.send(BackendEvent::Status(status)).is_err() {
            break;
        }
    }
}
