use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread,
    time::Duration,
};

use gtk4::{
    Application, ApplicationWindow, Button, DragSource, DropTarget, Entry, GestureClick, Image,
    Label, ListBox, Orientation, Popover, ScrolledWindow, TextView, gdk,
    glib::{self, ControlFlow},
    prelude::*,
};
use iron_file_common::{
    browse,
    config::{ConfigStore, Profile, SidebarLocation},
    ensure_backend, proto,
};
use proto::{BrowseResponse, browse_response::Payload};
use tokio::runtime::Runtime;

struct ConfigState {
    store: ConfigStore,
    profiles: Vec<Profile>,
    active_profile: Option<PathBuf>,
}

fn main() {
    if let Ok(runtime) = Runtime::new() {
        let _ = runtime.block_on(ensure_backend());
    }
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
    let config = Rc::new(RefCell::new(load_config_state()));
    let status = Label::new(Some("Connecting to backend"));
    let address = Entry::new();
    let initial_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    address.set_text(&initial_path.display().to_string());
    let open_button = Button::from_icon_name("folder-open-symbolic");
    open_button.set_tooltip_text(Some("Open path"));
    let up_button = Button::from_icon_name("go-up-symbolic");
    up_button.set_tooltip_text(Some("Parent folder"));
    let file_list = ListBox::new();
    let sidebar = ListBox::new();
    sidebar.set_selection_mode(gtk4::SelectionMode::None);
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
    let worker_sidebar = sidebar.clone();
    let worker_content = content_view.buffer();
    let worker_sender = response_sender.clone();
    let worker_config = config.clone();
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
                    &worker_config,
                    &worker_sidebar,
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
    let sidebar_view = ScrolledWindow::builder()
        .child(&sidebar)
        .width_request(180)
        .vexpand(true)
        .build();
    let main_area = gtk4::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .vexpand(true)
        .build();
    main_area.append(&sidebar_view);
    main_area.append(&panes);
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
    content.append(&main_area);
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Iron File")
        .default_width(900)
        .default_height(600)
        .child(&content)
        .build();
    window.present();

    replace_sidebar_entries(&sidebar, &config, &response_sender);

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
    config: &Rc<RefCell<ConfigState>>,
    sidebar: &ListBox,
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
            replace_entries(list, sender, directory.entries, config, sidebar);
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
    config: &Rc<RefCell<ConfigState>>,
    sidebar: &ListBox,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    for entry in entries {
        let icon_name = if entry.is_directory {
            "folder-symbolic"
        } else {
            "text-x-generic-symbolic"
        };
        let row = gtk4::Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .build();
        row.append(&Image::from_icon_name(icon_name));
        row.append(&Label::new(Some(&entry.name)));
        let button = Button::builder().child(&row).build();
        let path = PathBuf::from(&entry.path);
        let entry_sender = sender.clone();
        let open_path = path.clone();
        button.connect_clicked(move |_| request_path(&entry_sender, open_path.clone()));
        if entry.is_directory {
            add_folder_context_menu(&button, path, config, sender, sidebar);
        }
        list.append(&button);
    }
}

fn load_config_state() -> ConfigState {
    let store = ConfigStore::from_environment();
    let mut profiles = store.profiles().unwrap_or_default();
    if profiles.is_empty() {
        if let Ok(profile) = store.create_profile("Default") {
            profiles.push(profile);
        }
    }
    let active_profile = store
        .active_profile()
        .ok()
        .flatten()
        .filter(|path| profiles.iter().any(|profile| &profile.path == path))
        .or_else(|| profiles.first().map(|profile| profile.path.clone()));
    ConfigState {
        store,
        profiles,
        active_profile,
    }
}

fn active_sidebar_locations(config: &Rc<RefCell<ConfigState>>) -> Vec<SidebarLocation> {
    let config = config.borrow();
    config
        .active_profile
        .as_deref()
        .and_then(|path| config.profiles.iter().find(|profile| profile.path == path))
        .map(|profile| profile.sidebar_locations.clone())
        .unwrap_or_default()
}

fn save_sidebar_locations(
    config: &Rc<RefCell<ConfigState>>,
    locations: Vec<SidebarLocation>,
) -> Result<(), String> {
    let (store, profile) = {
        let config = config.borrow();
        let profile = config
            .active_profile
            .as_deref()
            .and_then(|path| config.profiles.iter().find(|profile| profile.path == path))
            .cloned()
            .ok_or_else(|| "No active configuration profile".to_owned())?;
        (config.store.clone(), profile)
    };
    let saved = store.save_sidebar_locations(&profile, locations)?;
    let saved_path = saved.path.clone();
    let mut config = config.borrow_mut();
    if let Some(index) = config
        .profiles
        .iter()
        .position(|profile| profile.path == saved_path)
    {
        config.profiles[index] = saved;
    } else {
        config.profiles.push(saved);
    }
    config.active_profile = Some(saved_path.clone());
    config.store.set_active_profile(&saved_path)
}

fn replace_sidebar_entries(
    sidebar: &ListBox,
    config: &Rc<RefCell<ConfigState>>,
    sender: &Sender<Result<BrowseResponse, String>>,
) {
    while let Some(child) = sidebar.first_child() {
        sidebar.remove(&child);
    }
    for location in active_sidebar_locations(config) {
        let row = gtk4::Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(8)
            .margin_end(8)
            .build();
        row.append(&Image::from_icon_name(sidebar_icon_name(&location)));
        row.append(&Label::new(Some(&location.label)));
        let button = Button::builder().child(&row).build();
        let path = location.path.clone();
        let open_sender = sender.clone();
        button.connect_clicked(move |_| request_path(&open_sender, path.clone()));

        let drag_source = DragSource::builder().actions(gdk::DragAction::MOVE).build();
        let drag_path = location.path.display().to_string();
        drag_source.connect_prepare(move |_, _, _| {
            Some(gdk::ContentProvider::for_value(&drag_path.to_value()))
        });
        button.add_controller(drag_source);

        let drop_target = DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
        let target_path = location.path.clone();
        let drop_config = config.clone();
        let drop_sidebar = sidebar.clone();
        let drop_sender = sender.clone();
        drop_target.connect_drop(move |_, value, _, _| {
            let Ok(source) = value.get::<String>() else {
                return false;
            };
            let source = PathBuf::from(source);
            if source == target_path {
                return false;
            }
            let mut locations = active_sidebar_locations(&drop_config);
            let Some(source_index) = locations
                .iter()
                .position(|location| location.path == source)
            else {
                return false;
            };
            let moved = locations.remove(source_index);
            let Some(target_index) = locations
                .iter()
                .position(|location| location.path == target_path)
            else {
                return false;
            };
            locations.insert(target_index, moved);
            if save_sidebar_locations(&drop_config, locations).is_ok() {
                replace_sidebar_entries(&drop_sidebar, &drop_config, &drop_sender);
                true
            } else {
                false
            }
        });
        button.add_controller(drop_target);
        sidebar.append(&button);
    }
}

fn add_folder_context_menu(
    button: &Button,
    path: PathBuf,
    config: &Rc<RefCell<ConfigState>>,
    sender: &Sender<Result<BrowseResponse, String>>,
    sidebar: &ListBox,
) {
    let gesture = GestureClick::new();
    gesture.set_button(3);
    let menu_button = button.clone();
    let menu_config = config.clone();
    let menu_sender = sender.clone();
    let menu_sidebar = sidebar.clone();
    gesture.connect_pressed(move |_, _, x, y| {
        let locations = active_sidebar_locations(&menu_config);
        let is_in_sidebar = locations.iter().any(|location| location.path == path);
        let action = Button::with_label(if is_in_sidebar {
            "Remove from sidebar"
        } else {
            "Add to sidebar"
        });
        let popover = Popover::new();
        let content = gtk4::Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(6)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .build();
        content.append(&action);
        popover.set_child(Some(&content));
        popover.set_parent(&menu_button);
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        let action_config = menu_config.clone();
        let action_sidebar = menu_sidebar.clone();
        let action_sender = menu_sender.clone();
        let action_path = path.clone();
        action.connect_clicked(move |_| {
            let mut locations = active_sidebar_locations(&action_config);
            if is_in_sidebar {
                locations.retain(|location| location.path != action_path);
            } else {
                let label = action_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| action_path.display().to_string());
                locations.push(SidebarLocation {
                    label,
                    path: action_path.clone(),
                });
            }
            if save_sidebar_locations(&action_config, locations).is_ok() {
                replace_sidebar_entries(&action_sidebar, &action_config, &action_sender);
            }
        });
        popover.popup();
    });
    button.add_controller(gesture);
}

fn sidebar_icon_name(location: &SidebarLocation) -> &'static str {
    match location.label.as_str() {
        "Home" => "user-home-symbolic",
        "Downloads" => "folder-download-symbolic",
        "Pictures" => "folder-pictures-symbolic",
        _ => "folder-symbolic",
    }
}
