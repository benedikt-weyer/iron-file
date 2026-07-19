use std::{
    cell::RefCell,
    rc::Rc,
    sync::mpsc::{Receiver, Sender, TryRecvError},
    time::Duration,
};

use gtk4::{
    Application, ApplicationWindow, Button, Label, Orientation,
    glib::{self, ControlFlow},
    prelude::*,
};
use iron_file::{BackendEvent, GuiEvent, start_backend};

fn main() {
    let (gui_sender, backend_receiver, worker_thread) = start_backend();
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
    let status = Label::new(Some("Waiting for input"));
    let button = Button::with_label("Send message to background worker");
    let button_status = status.clone();

    button.connect_clicked(move |_| {
        if gui_sender.send(GuiEvent::ButtonPressed).is_ok() {
            button_status.set_text("Sent button press to the background worker");
        } else {
            button_status.set_text("Background worker is no longer available");
        }
    });

    let worker_status = status.clone();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        loop {
            match backend_receiver.try_recv() {
                Ok(BackendEvent::Status(message)) => worker_status.set_text(&message),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    worker_status.set_text("Background worker disconnected");
                    return ControlFlow::Break;
                }
            }
        }

        ControlFlow::Continue
    });

    let content = gtk4::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(16)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    content.append(&Label::new(Some("GUI and background worker communication")));
    content.append(&button);
    content.append(&status);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Iron File")
        .default_width(480)
        .default_height(220)
        .child(&content)
        .build();
    window.present();
}
