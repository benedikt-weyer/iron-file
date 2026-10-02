mod backdrop_blur;
mod browser_view;
mod commands;
mod navigation;
mod preferences_view;
mod profile_settings;
mod selection;
mod settings;
mod sidebar;
mod startup;
mod style;
mod types;
mod update_browser;
mod update_context;
mod update_loading;
mod update_preferences;
mod utilities;

use startup::*;
use style::*;
use types::*;
use utilities::*;

use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    env,
    ffi::OsString,
    fs,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    sync::atomic::{AtomicU8, Ordering},
    time::{Duration, Instant},
};

use iced::{
    Background, Border, Color, Element, Font, Gradient, Length, Point, Shadow, Subscription, Task,
    Theme, Vector,
    gradient::Linear,
    keyboard, mouse, task,
    widget::{
        Id, Space, button as button_style, checkbox, container, image, mouse_area, opaque,
        operation, pick_list, radio, responsive, scrollable, slider, stack, svg, text, text_input,
        toggler, tooltip,
    },
    window,
};
use iconflow::{Pack, Size, Style, fonts, try_icon};
use iron_file_common::{
    browse_with_thumbnails, compress_entries,
    config::{
        BrowserLayout, BrowserSettings, ColorMode, ConfigStore, ContextMenuBlurKernelSize,
        ContextMenuItem, EntrySortOrder, FolderSortOverride, KeyboardShortcutAction, NameAlignment,
        Profile, QuickToolbarItem, SidebarLocation,
    },
    copy_entries, create_entry, create_symlinks, create_thumbnail, delete_entries, ensure_backend,
    extract_archives, inspect_entry, pipe_backend_logs, proto, rename_entry, restart_backend,
    search_directory, stream_directory,
};
use proto::{BrowseResponse, browse_response::Payload};
use serde::Deserialize;
use tokio::runtime::Runtime;

const DETACHED_ENV: &str = "IRON_FILE_DETACHED";
const NAVIGATION_CONTROL_HEIGHT: f32 = 32.0;
const DRAG_START_THRESHOLD: f32 = 5.0;

fn rename_name_input_id() -> Id {
    Id::new("rename-entry-name-input")
}

fn create_name_input_id() -> Id {
    Id::new("create-entry-name-input")
}
static BORDER_RADIUS: AtomicU8 = AtomicU8::new(6);

/// `vendor/winit` is patched to drop its client-side-decoration fallback, so
/// this only requests server-side decoration; the compositor's
/// xdg-decoration choice decides the outcome either way.
const SERVER_SIDE_ONLY_DECORATIONS: bool = true;

fn shortcut_key_name(key: &keyboard::Key) -> Option<String> {
    match key.as_ref() {
        keyboard::Key::Named(key) => Some(format!("{key:?}")),
        keyboard::Key::Character(key) if !key.is_empty() => Some(key.to_uppercase()),
        keyboard::Key::Character(_) | keyboard::Key::Unidentified => None,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut startup = startup_options();
    startup.initial_path = startup.initial_path.map(resolve_initial_path);
    if !startup.follow_logs && startup.picker.is_none() && !is_detached() {
        detach()?;
        return Ok(());
    }

    if let Ok(runtime) = Runtime::new() {
        let _ = runtime.block_on(ensure_backend());
    }
    let startup_follow_logs = startup.follow_logs;
    let startup_path = startup.initial_path.clone();
    let startup_picker = startup.picker.clone();
    iced::application(
        move || {
            let gui = Gui::new(
                startup_follow_logs,
                startup_path.clone(),
                startup_picker.clone(),
            );
            let task = gui.load_initial_directory();
            (gui, task)
        },
        Gui::update,
        Gui::view,
    )
    .title("Iron File")
    .theme(Gui::theme)
    .subscription(Gui::subscription)
    .window(window::Settings {
        transparent: true,
        decorations: SERVER_SIDE_ONLY_DECORATIONS,
        platform_specific: window::settings::PlatformSpecific {
            // Must match iron-file.desktop so the desktop shell can resolve the dock icon.
            application_id: "iron-file".into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .run()?;
    Ok(())
}

impl Gui {
    fn subscription(&self) -> Subscription<Message> {
        iced::event::listen_raw(|event, _, _| match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                Some(Message::LeftMouseButtonPressed)
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                Some(Message::LeftMouseButtonReleased)
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                Some(Message::WindowCursorMoved(position))
            }
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                Some(Message::ModifiersChanged(modifiers))
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, .. })
                if key == keyboard::Key::Named(keyboard::key::Named::Escape) =>
            {
                Some(Message::EscapePressed)
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                if modifiers.command() =>
            {
                match key.as_ref() {
                    keyboard::Key::Character("c" | "C") => Some(Message::ExecuteBrowserCommand(
                        BrowserCommand::CopySelection,
                    )),
                    keyboard::Key::Character("x" | "X") => {
                        Some(Message::ExecuteBrowserCommand(BrowserCommand::CutSelection))
                    }
                    keyboard::Key::Character("v" | "V") => {
                        Some(Message::ExecuteBrowserCommand(BrowserCommand::Paste))
                    }
                    keyboard::Key::Character("f" | "F") => {
                        Some(Message::ShortcutPressed("Ctrl+F".into()))
                    }
                    _ => None,
                }
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, .. })
                if matches!(
                    key.as_ref(),
                    keyboard::Key::Named(
                        keyboard::key::Named::ArrowLeft
                            | keyboard::key::Named::ArrowRight
                            | keyboard::key::Named::ArrowUp
                            | keyboard::key::Named::ArrowDown
                    )
                ) =>
            {
                match key.as_ref() {
                    keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
                        Some(Message::ArrowKeyPressed(SelectionDirection::Left))
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
                        Some(Message::ArrowKeyPressed(SelectionDirection::Right))
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                        Some(Message::ArrowKeyPressed(SelectionDirection::Up))
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                        Some(Message::ArrowKeyPressed(SelectionDirection::Down))
                    }
                    _ => unreachable!(),
                }
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, .. })
                if key == keyboard::Key::Named(keyboard::key::Named::Enter) =>
            {
                Some(Message::ActivateDeleteDialogAction)
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, .. })
                if key == keyboard::Key::Named(keyboard::key::Named::Delete) =>
            {
                Some(Message::ExecuteBrowserCommand(
                    BrowserCommand::DeleteSelection,
                ))
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => {
                shortcut_key_name(&key).map(Message::ShortcutPressed)
            }
            _ => None,
        })
    }

    fn new(
        follow_logs: bool,
        initial_path: Option<PathBuf>,
        picker: Option<PickerOptions>,
    ) -> Self {
        let save_file_name = picker
            .as_ref()
            .and_then(|picker| picker.save_file_name.clone());
        let original_save_file_name = save_file_name.clone();
        let config_store = ConfigStore::from_environment();
        let directory_path = initial_path
            .or_else(|| {
                picker
                    .is_some()
                    .then(|| config_store.last_picker_directory().ok().flatten())
                    .flatten()
            })
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let mut profiles = config_store.profiles().unwrap_or_default();
        if profiles.is_empty() {
            if let Ok(profile) = config_store.create_profile("Default") {
                profiles.push(profile);
            }
        }
        let active_profile = config_store
            .active_profile()
            .ok()
            .flatten()
            .filter(|path| profiles.iter().any(|profile| &profile.path == path))
            .or_else(|| profiles.first().map(|profile| profile.path.clone()));
        let color_mode = active_profile
            .as_deref()
            .and_then(|path| profiles.iter().find(|profile| profile.path == path))
            .map(|profile| profile.color_mode)
            .unwrap_or_default();
        let theme = active_profile
            .as_deref()
            .and_then(|path| profiles.iter().find(|profile| profile.path == path))
            .map(|profile| profile.theme.clone())
            .unwrap_or_else(iron_file_common::config::default_theme_settings);
        set_border_radius(theme.border_radius);
        Self {
            follow_logs,
            picker,
            save_file_name,
            original_save_file_name,
            address: directory_path.display().to_string(),
            directory_path,
            entries: Vec::new(),
            drives: Vec::new(),
            mounts: Vec::new(),
            content: String::new(),
            status: "Connecting to backend".into(),
            editing_address: false,
            view: View::Browser,
            config_store,
            profiles,
            active_profile,
            new_profile_name: String::new(),
            color_mode,
            light_accent_input: theme.light_highlight,
            dark_accent_input: theme.dark_highlight,
            accent_picker: None,
            context_entry: None,
            show_performance_debugger: false,
            folder_load_started: None,
            last_folder_load_duration: None,
            folder_load_performance: None,
            pending_info: None,
            pointer_position: Point::ORIGIN,
            context_position: Point::ORIGIN,
            dragging_sidebar_location: None,
            sidebar_drop_target: None,
            sidebar_drop_at_end: false,
            dragging_entries: None,
            pending_drag: None,
            entry_drop_target: None,
            hovered_entry: None,
            window_cursor_position: Point::ORIGIN,
            last_entry_click: None,
            terminal_recommendations: recommended_terminal_commands(),
            history: Vec::new(),
            history_index: None,
            sidebar_resize: None,
            icon_themes: available_icon_themes(),
            entry_icons: HashMap::new(),
            entry_icon_cache: HashMap::new(),
            thumbnail_handles: HashMap::new(),
            pending_thumbnail_paths: Vec::new(),
            selected_entries: HashSet::new(),
            paste_buffer: None,
            pending_delete: None,
            delete_confirm_selected: false,
            pending_create: None,
            create_entry_name: String::new(),
            pending_rename: None,
            rename_entry_name: String::new(),
            pending_profile_reset: false,
            pending_compression: false,
            compression_level: 6,
            compression_type: ArchiveCompression::Deflate,
            selection_anchor: None,
            modifiers: keyboard::Modifiers::default(),
            browser_pointer: Point::ORIGIN,
            browser_scroll_offset: scrollable::AbsoluteOffset::default(),
            rectangle_selection: None,
            search: None,
            tile_columns: Rc::new(Cell::new(1)),
        }
    }

    fn load_initial_directory(&self) -> Task<Message> {
        let path = self.directory_path.clone();
        let thumbnail_directory = self.active_browser_settings().thumbnail_location;
        Task::batch(
            fonts()
                .iter()
                .map(|font| iced::font::load(font.bytes).map(Message::IconFontLoaded))
                .chain(std::iter::once(Task::perform(
                    browse_with_thumbnails(path, Some(thumbnail_directory)),
                    |result| Message::BrowseFinished {
                        result,
                        history: HistoryRequest::Initial,
                    },
                )))
                .chain(std::iter::once(Task::perform(
                    load_mounts(),
                    Message::MountsLoaded,
                )))
                .chain(
                    self.follow_logs
                        .then(|| Task::perform(pipe_backend_logs(), Message::BackendLogPipeEnded)),
                ),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        self.update_browser(message)
    }

    fn view(&self) -> Element<'_, Message> {
        let content = match self.view {
            View::Browser => self.browser_view(),
            View::Preferences => self.preferences_view(),
        };
        match self.drag_ghost_view() {
            Some(ghost) => stack![content, ghost].into(),
            None => content,
        }
    }
}
