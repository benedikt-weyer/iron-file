mod backdrop_blur;
mod browser_view;
mod navigation;
mod preferences_view;
mod profile_settings;
mod sidebar;
mod startup;
mod types;
mod utilities;

use startup::*;
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
        operation, pick_list, radio, responsive, row, scrollable, slider, stack, svg, text,
        text_input, toggler, tooltip,
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

fn border_radius() -> f32 {
    f32::from(BORDER_RADIUS.load(Ordering::Relaxed))
}

fn set_border_radius(radius: u8) {
    BORDER_RADIUS.store(radius.min(8), Ordering::Relaxed);
}

fn button<'a, Message>(
    content: impl Into<Element<'a, Message>>,
) -> iced::widget::Button<'a, Message> {
    iced::widget::button(content).style(rounded_button_style)
}

fn rounded_button_style(theme: &Theme, status: button_style::Status) -> button_style::Style {
    let base = button_style::primary(theme, status);
    button_style::Style {
        border: Border {
            radius: border_radius().into(),
            ..base.border
        },
        ..base
    }
}

fn rounded_text_button_style(theme: &Theme, status: button_style::Status) -> button_style::Style {
    let base = button_style::text(theme, status);
    button_style::Style {
        border: Border {
            radius: border_radius().into(),
            ..base.border
        },
        ..base
    }
}

fn rounded_text_input_style(
    theme: &Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    let base = iced::widget::text_input::default(theme, status);
    iced::widget::text_input::Style {
        border: Border {
            radius: border_radius().into(),
            ..base.border
        },
        ..base
    }
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
        match message {
            Message::AddressChanged(address) => {
                self.address = address;
                Task::none()
            }
            Message::StartAddressEdit => {
                self.editing_address = true;
                Task::none()
            }
            Message::CancelAddressEdit => {
                if !self.editing_address {
                    return Task::none();
                }
                self.address = self.directory_path.display().to_string();
                self.editing_address = false;
                Task::none()
            }
            Message::EscapePressed => {
                if self.context_entry.is_some() {
                    self.context_entry = None;
                } else if self.pending_info.is_some() {
                    self.pending_info = None;
                } else if self.pending_rename.is_some() {
                    self.pending_rename = None;
                    self.rename_entry_name.clear();
                } else if self.pending_create.is_some() {
                    self.pending_create = None;
                    self.create_entry_name.clear();
                } else if self.editing_address {
                    self.address = self.directory_path.display().to_string();
                    self.editing_address = false;
                }
                Task::none()
            }
            Message::OpenAddress => self.open_path(PathBuf::from(&self.address)),
            Message::OpenPath(path) => self.open_path(path),
            Message::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers;
                Task::none()
            }
            Message::StartRectangleSelection => {
                self.rectangle_selection = Some(RectangleSelection {
                    start: self.browser_pointer,
                    end: self.browser_pointer,
                    initial_selection: if self.modifiers.command() {
                        self.selected_entries.clone()
                    } else {
                        HashSet::new()
                    },
                });
                if !self.modifiers.command() {
                    self.selected_entries.clear();
                    self.selection_anchor = None;
                }
                Task::none()
            }
            Message::RectanglePointerMoved(position) => {
                self.browser_pointer = position;
                if let Some(selection) = &mut self.rectangle_selection {
                    selection.end = position;
                }
                self.update_rectangle_selection();
                Task::none()
            }
            Message::FinishRectangleSelection => {
                self.rectangle_selection = None;
                self.pending_drag = None;
                if let Some(target) = self.entry_drop_target.take() {
                    return self.drop_dragged_entries(target);
                }
                self.dragging_entries = None;
                Task::none()
            }
            Message::BrowserScrolled(viewport) => {
                self.browser_scroll_offset = viewport.absolute_offset();
                self.update_rectangle_selection();
                Task::none()
            }
            Message::NavigateBack => self.navigate_history(-1),
            Message::NavigateForward => self.navigate_history(1),
            Message::EntryClicked { path, is_directory } => {
                self.handle_entry_click(path, is_directory)
            }
            Message::EntryHovered { path, is_directory } => {
                self.hovered_entry = Some((path.clone(), is_directory));
                if is_directory && self.dragging_entries.is_some() {
                    self.entry_drop_target = Some(path);
                }
                Task::none()
            }
            Message::EntryUnhovered(path) => {
                if self.hovered_entry.as_ref().is_some_and(|(p, _)| p == &path) {
                    self.hovered_entry = None;
                }
                if self.entry_drop_target.as_ref() == Some(&path) {
                    self.entry_drop_target = None;
                }
                Task::none()
            }
            Message::LeftMouseButtonPressed => {
                if self.view == View::Browser && !self.editing_address {
                    if let Some((path, _)) = self.hovered_entry.clone() {
                        let entries = if self.selected_entries.contains(&path) {
                            self.selected_entries.iter().cloned().collect()
                        } else {
                            vec![path]
                        };
                        self.pending_drag = Some((self.window_cursor_position, entries));
                    }
                }
                Task::none()
            }
            Message::LeftMouseButtonReleased => {
                self.pending_drag = None;
                if let Some(target) = self.entry_drop_target.take() {
                    return self.drop_dragged_entries(target);
                }
                self.dragging_entries = None;
                Task::none()
            }
            Message::WindowCursorMoved(position) => {
                self.window_cursor_position = position;
                if let Some((start, entries)) = &self.pending_drag {
                    let delta = *start - position;
                    if delta.x.hypot(delta.y) > DRAG_START_THRESHOLD {
                        self.dragging_entries = Some(entries.clone());
                        self.pending_drag = None;
                    }
                }
                Task::none()
            }
            Message::ConfirmPicker => self.confirm_picker(),
            Message::CancelPicker => self.close_window(),
            Message::SaveFileNameChanged(name) => {
                self.save_file_name = Some(name);
                Task::none()
            }
            Message::ResetSaveFileName => {
                self.save_file_name = self.original_save_file_name.clone();
                Task::none()
            }
            Message::RefreshDirectory => self.refresh_directory(),
            Message::CloneWindow => {
                self.status = match clone_window(&self.directory_path) {
                    Ok(()) => "Opened new window".into(),
                    Err(error) => error,
                };
                Task::none()
            }
            Message::DuplicateContextEntry(path) => {
                let Some(parent) = path.parent().map(Path::to_path_buf) else {
                    self.status = format!("Could not duplicate {}", path.display());
                    return Task::none();
                };
                Task::perform(copy_entries(vec![path], parent), Message::FileCopyFinished)
            }
            Message::ExecuteBrowserCommand(command) => self.execute_browser_command(command),
            Message::FileCopyFinished(result) => match result {
                Ok(paths) => {
                    self.status = format!("Copied {} item(s)", paths.len());
                    self.open_path(self.directory_path.clone())
                }
                Err(error) => {
                    self.status = format!("Copy failed: {error}");
                    Task::none()
                }
            },
            Message::CutPasteFinished { sources, result } => match result {
                Ok(paths) => {
                    self.paste_buffer = None;
                    self.status = format!("Moved {} item(s)", paths.len());
                    Task::perform(delete_entries(sources), Message::FileDeleteFinished)
                }
                Err(error) => {
                    self.status = format!("Move failed: {error}");
                    Task::none()
                }
            },
            Message::ArchiveFinished { action, result } => match result {
                Ok(paths) => {
                    self.status = format!("{action} {} item(s)", paths.len());
                    self.open_path(self.directory_path.clone())
                }
                Err(error) => {
                    self.status = format!("{action} failed: {error}");
                    Task::none()
                }
            },
            Message::CompressionLevelChanged(level) => {
                self.compression_level = level;
                Task::none()
            }
            Message::CompressionTypeSelected(compression_type) => {
                self.compression_type = compression_type;
                Task::none()
            }
            Message::ConfirmCompression => {
                self.pending_compression = false;
                self.execute_compression()
            }
            Message::CancelCompression => {
                self.pending_compression = false;
                Task::none()
            }
            Message::ConfirmDelete => {
                let Some(paths) = self.pending_delete.take() else {
                    return Task::none();
                };
                self.delete_confirm_selected = false;
                self.status = format!("Deleting {} item(s)...", paths.len());
                Task::perform(delete_entries(paths), Message::FileDeleteFinished)
            }
            Message::CancelDelete => {
                self.pending_delete = None;
                self.delete_confirm_selected = false;
                Task::none()
            }
            Message::SelectDeleteDialogAction(delete) => {
                if self.pending_delete.is_some() {
                    self.delete_confirm_selected = delete;
                }
                Task::none()
            }
            Message::ActivateDeleteDialogAction => {
                if self.pending_delete.is_some() {
                    if self.delete_confirm_selected {
                        self.update(Message::ConfirmDelete)
                    } else {
                        self.update(Message::CancelDelete)
                    }
                } else {
                    Task::none()
                }
            }
            Message::ArrowKeyPressed(direction) => {
                if self.pending_delete.is_some() {
                    match direction {
                        SelectionDirection::Left => {
                            self.update(Message::SelectDeleteDialogAction(false))
                        }
                        SelectionDirection::Right => {
                            self.update(Message::SelectDeleteDialogAction(true))
                        }
                        SelectionDirection::Up | SelectionDirection::Down => Task::none(),
                    }
                } else {
                    self.move_selected_entry(direction);
                    Task::none()
                }
            }
            Message::FileDeleteFinished(result) => match result {
                Ok(paths) => {
                    self.status = format!("Deleted {} item(s)", paths.len());
                    self.selected_entries.clear();
                    self.selection_anchor = None;
                    self.open_path(self.directory_path.clone())
                }
                Err(error) => {
                    self.status = format!("Delete failed: {error}");
                    Task::none()
                }
            },
            Message::RequestCreateEntry {
                parent,
                is_directory,
            } => {
                self.context_entry = None;
                self.pending_create = Some((parent, is_directory));
                self.create_entry_name.clear();
                let id = create_name_input_id();
                operation::focus(id)
            }
            Message::CreateEntryNameChanged(name) => {
                self.create_entry_name = name;
                Task::none()
            }
            Message::ConfirmCreateEntry => {
                let Some((parent, is_directory)) = self.pending_create.take() else {
                    return Task::none();
                };
                let name = self.create_entry_name.trim().to_owned();
                if name.is_empty() {
                    self.pending_create = Some((parent, is_directory));
                    self.status = "Enter a name".into();
                    return Task::none();
                }
                self.status = format!("Creating {name}...");
                Task::perform(
                    create_entry(parent, name, is_directory),
                    Message::EntryCreated,
                )
            }
            Message::CancelCreateEntry => {
                self.pending_create = None;
                self.create_entry_name.clear();
                Task::none()
            }
            Message::EntryCreated(result) => match result {
                Ok(path) => {
                    self.create_entry_name.clear();
                    self.status = format!("Created {}", path.display());
                    self.open_path(self.directory_path.clone())
                }
                Err(error) => {
                    self.status = format!("Create failed: {error}");
                    Task::none()
                }
            },
            Message::RequestRenameEntry(path) => {
                self.context_entry = None;
                self.rename_entry_name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.pending_rename = Some(path);
                let id = rename_name_input_id();
                Task::batch([operation::focus(id.clone()), operation::select_all(id)])
            }
            Message::RenameEntryNameChanged(name) => {
                self.rename_entry_name = name;
                Task::none()
            }
            Message::ConfirmRenameEntry => {
                let Some(path) = self.pending_rename.take() else {
                    return Task::none();
                };
                let name = self.rename_entry_name.trim().to_owned();
                if name.is_empty() {
                    self.pending_rename = Some(path);
                    self.status = "Enter a name".into();
                    return Task::none();
                }
                self.status = format!("Renaming {}...", path.display());
                Task::perform(rename_entry(path, name), Message::EntryRenamed)
            }
            Message::CancelRenameEntry => {
                self.pending_rename = None;
                self.rename_entry_name.clear();
                Task::none()
            }
            Message::EntryRenamed(result) => match result {
                Ok(path) => {
                    self.rename_entry_name.clear();
                    self.status = format!("Renamed to {}", path.display());
                    self.open_path(self.directory_path.clone())
                }
                Err(error) => {
                    self.status = format!("Rename failed: {error}");
                    Task::none()
                }
            },
            Message::OpenParent => {
                let parent = self.directory_path.parent().map(|path| path.to_path_buf());
                parent
                    .map(|path| self.open_path(path))
                    .unwrap_or_else(Task::none)
            }
            Message::ShowSearch => {
                self.search = Some(SearchState {
                    root: self.directory_path.clone(),
                    query: String::new(),
                    depth: SearchDepth::CurrentFolder,
                    in_progress: false,
                    pending_request: None,
                    cancel_handle: None,
                });
                self.status = format!("Search in {}", self.directory_path.display());
                Task::none()
            }
            Message::CloseSearch => {
                self.cancel_search();
                self.search = None;
                self.refresh_directory()
            }
            Message::SearchQueryChanged(query) => {
                if let Some(search) = &mut self.search {
                    search.query = query;
                }
                self.run_search()
            }
            Message::SearchDepthSelected(depth) => {
                if let Some(search) = &mut self.search {
                    search.depth = depth;
                }
                self.run_search()
            }
            Message::RunSearch => self.run_search(),
            Message::CancelSearch => {
                self.cancel_search();
                self.status = "Search cancelled".into();
                Task::none()
            }
            Message::SearchFinished {
                root,
                query,
                depth,
                requested_depth,
                result,
            } => {
                let Some(search) = self.search.as_mut() else {
                    return Task::none();
                };
                let is_current = search.root == root
                    && search.query == query
                    && search.depth == depth
                    && search
                        .pending_request
                        .is_some_and(|(pending_depth, _)| pending_depth == requested_depth);
                if !is_current {
                    return Task::none();
                }
                match result {
                    Ok(mut entries) => {
                        let sort_order = self
                            .folder_sort_override(&self.directory_path)
                            .unwrap_or(self.active_browser_settings().sort_order);
                        sort_entries(&mut entries, sort_order);
                        let found = entries.len();
                        self.entries = entries;
                        self.selected_entries.clear();
                        self.selection_anchor = None;

                        let previous_count = self
                            .search
                            .as_ref()
                            .and_then(|s| s.pending_request)
                            .and_then(|(_, previous_count)| previous_count);
                        let keep_going = depth == SearchDepth::Progressive
                            && requested_depth < PROGRESSIVE_SEARCH_MAX_DEPTH
                            && previous_count != Some(found);
                        if keep_going {
                            self.status =
                                format!("{found} result(s) so far (depth {requested_depth})...");
                            return self.dispatch_search(requested_depth + 1, Some(found));
                        }

                        if let Some(search) = self.search.as_mut() {
                            search.in_progress = false;
                            search.pending_request = None;
                            search.cancel_handle = None;
                        }
                        self.status = format!("{found} search result(s)");
                        Task::none()
                    }
                    Err(error) => {
                        if let Some(search) = self.search.as_mut() {
                            search.in_progress = false;
                            search.pending_request = None;
                            search.cancel_handle = None;
                        }
                        self.status = format!("Search failed: {error}");
                        Task::none()
                    }
                }
            }
            Message::ShowBrowser => {
                self.view = View::Browser;
                Task::none()
            }
            Message::ShowPreferences => {
                self.view = View::Preferences;
                Task::none()
            }
            Message::SelectProfile(path) => {
                self.select_profile(path);
                Task::none()
            }
            Message::NewProfileNameChanged(name) => {
                self.new_profile_name = name;
                Task::none()
            }
            Message::CreateProfile => {
                self.create_profile();
                Task::none()
            }
            Message::RequestProfileReset => {
                self.pending_profile_reset = true;
                Task::none()
            }
            Message::ConfirmProfileReset => {
                self.pending_profile_reset = false;
                self.reset_active_profile();
                Task::none()
            }
            Message::CancelProfileReset => {
                self.pending_profile_reset = false;
                Task::none()
            }
            Message::ResetPreference(option) => {
                self.reset_preference(option);
                Task::none()
            }
            Message::ColorModeSelected(color_mode) => {
                self.save_color_mode(color_mode);
                Task::none()
            }
            Message::BackgroundOpacityChanged(opacity) => {
                self.save_background_opacity(opacity);
                Task::none()
            }
            Message::ContextMenuBlurStrengthChanged(strength) => {
                self.save_context_menu_blur_strength(strength);
                Task::none()
            }
            Message::ContextMenuBlurKernelSizeChanged(kernel_size) => {
                self.save_context_menu_blur_kernel_size(kernel_size);
                Task::none()
            }
            Message::ContextMenuItemToggled {
                item,
                is_directory,
                enabled,
            } => {
                let browser = self.active_browser_settings();
                let mut items = if is_directory {
                    browser.folder_context_menu_items
                } else {
                    browser.file_context_menu_items
                };
                if enabled {
                    if !items.contains(&item) {
                        items.push(item);
                    }
                } else {
                    items.retain(|configured_item| *configured_item != item);
                }
                self.save_context_menu_items(is_directory, items);
                Task::none()
            }
            Message::MoveContextMenuItem {
                item,
                is_directory,
                move_up,
            } => {
                let browser = self.active_browser_settings();
                let mut items = if is_directory {
                    browser.folder_context_menu_items
                } else {
                    browser.file_context_menu_items
                };
                if let Some(index) = items
                    .iter()
                    .position(|configured_item| *configured_item == item)
                {
                    let target = if move_up {
                        index.checked_sub(1)
                    } else {
                        (index + 1 < items.len()).then_some(index + 1)
                    };
                    if let Some(target) = target {
                        items.swap(index, target);
                        self.save_context_menu_items(is_directory, items);
                    }
                }
                Task::none()
            }
            Message::QuickToolbarItemToggled(item, enabled) => {
                let mut items = self.active_browser_settings().quick_toolbar_items;
                if enabled {
                    if !items.contains(&item) {
                        items.push(item);
                    }
                } else {
                    items.retain(|configured_item| *configured_item != item);
                }
                self.save_quick_toolbar_items(items);
                Task::none()
            }
            Message::MoveQuickToolbarItem(item, move_up) => {
                let mut items = self.active_browser_settings().quick_toolbar_items;
                if let Some(index) = items
                    .iter()
                    .position(|configured_item| *configured_item == item)
                {
                    let target = if move_up {
                        index.checked_sub(1)
                    } else {
                        (index + 1 < items.len()).then_some(index + 1)
                    };
                    if let Some(target) = target {
                        items.swap(index, target);
                        self.save_quick_toolbar_items(items);
                    }
                }
                Task::none()
            }
            Message::SortOrderSelected(sort_order) => {
                let mut browser = self.active_browser_settings();
                browser.sort_order = sort_order;
                let effective_sort_order = self
                    .folder_sort_override(&self.directory_path)
                    .unwrap_or(sort_order);
                sort_entries(&mut self.entries, effective_sort_order);
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::FolderSortOverrideSelected(selection) => {
                self.save_folder_sort_override(selection.sort_order());
                Task::none()
            }
            Message::KeyboardShortcutChanged { action, key } => {
                self.save_keyboard_shortcut(action, key);
                Task::none()
            }
            Message::ShortcutPressed(key) => {
                let action = self
                    .active_browser_settings()
                    .keyboard_shortcuts
                    .into_iter()
                    .find(|shortcut| shortcut.key.eq_ignore_ascii_case(&key))
                    .map(|shortcut| shortcut.action);
                match action {
                    Some(KeyboardShortcutAction::RenameSelection) => {
                        self.execute_browser_command(BrowserCommand::RenameSelection)
                    }
                    Some(KeyboardShortcutAction::SearchCurrentFolder) => {
                        self.update(Message::ShowSearch)
                    }
                    None => Task::none(),
                }
            }
            Message::BorderRadiusChanged(radius) => {
                self.save_border_radius(radius);
                Task::none()
            }
            Message::OpenAccentPicker(dark) => {
                let color = parse_color(if dark {
                    &self.dark_accent_input
                } else {
                    &self.light_accent_input
                })
                .unwrap_or(Color::BLACK);
                let (hue, saturation, value) = rgb_to_hsv(color);
                self.accent_picker = Some(AccentPickerState {
                    dark,
                    hue,
                    saturation,
                    value,
                });
                Task::none()
            }
            Message::AccentHueChanged(hue) => {
                if let Some(picker) = &mut self.accent_picker {
                    picker.hue = hue;
                }
                Task::none()
            }
            Message::AccentSaturationChanged(saturation) => {
                if let Some(picker) = &mut self.accent_picker {
                    picker.saturation = saturation;
                }
                Task::none()
            }
            Message::AccentValueChanged(value) => {
                if let Some(picker) = &mut self.accent_picker {
                    picker.value = value;
                }
                Task::none()
            }
            Message::ConfirmAccentPicker => {
                let Some(picker) = self.accent_picker.take() else {
                    return Task::none();
                };
                let color = hsv_color(picker.hue, picker.saturation, picker.value);
                self.save_accent_color(
                    picker.dark,
                    format!(
                        "#{:02x}{:02x}{:02x}",
                        (color.r * 255.0).round() as u8,
                        (color.g * 255.0).round() as u8,
                        (color.b * 255.0).round() as u8
                    ),
                );
                Task::none()
            }
            Message::CancelAccentPicker => {
                self.accent_picker = None;
                Task::none()
            }
            Message::BrowserLayoutSelected(layout) => {
                let mut browser = self.active_browser_settings();
                browser.layout = layout;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::NameAlignmentSelected(name_alignment) => {
                let mut browser = self.active_browser_settings();
                browser.name_alignment = name_alignment;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::SmoothScrollingToggled(smooth_scrolling) => {
                let mut browser = self.active_browser_settings();
                browser.smooth_scrolling = smooth_scrolling;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::ScrollStepChanged(scroll_step) => {
                let mut browser = self.active_browser_settings();
                browser.scroll_step = scroll_step.clamp(10, 200);
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::BrowserItemSizeChanged(item_size) => {
                let mut browser = self.active_browser_settings();
                browser.item_size = item_size;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::MaxNameLinesChanged(max_name_lines) => {
                let mut browser = self.active_browser_settings();
                browser.max_name_lines = max_name_lines.clamp(1, 5);
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::PreviewToggled(preview_enabled) => {
                let mut browser = self.active_browser_settings();
                browser.preview_enabled = preview_enabled;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::ToggleHiddenFiles => {
                let mut browser = self.active_browser_settings();
                browser.show_hidden_files = !browser.show_hidden_files;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::SingleClickFoldersToggled(single_click_opens_folders) => {
                let mut browser = self.active_browser_settings();
                browser.single_click_opens_folders = single_click_opens_folders;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::TerminalChoiceSelected(choice) => {
                let mut browser = self.active_browser_settings();
                browser.terminal_command = if choice == DEFAULT_TERMINAL_CHOICE {
                    "default".into()
                } else if choice == CUSTOM_TERMINAL_CHOICE {
                    if browser.terminal_command == "default"
                        || self
                            .terminal_recommendations
                            .contains(&browser.terminal_command)
                    {
                        String::new()
                    } else {
                        browser.terminal_command
                    }
                } else {
                    choice
                };
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::TerminalCommandChanged(terminal_command) => {
                let mut browser = self.active_browser_settings();
                browser.terminal_command = terminal_command;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::IconThemeSelected(icon_theme) => {
                let mut browser = self.active_browser_settings();
                browser.icon_theme = icon_theme;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::ThumbnailLocationChanged(thumbnail_location) => {
                let mut browser = self.active_browser_settings();
                browser.thumbnail_location = PathBuf::from(thumbnail_location);
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::StartSidebarResize => {
                self.sidebar_resize = Some((
                    self.pointer_position.x,
                    self.sidebar_width(),
                    self.sidebar_width(),
                ));
                Task::none()
            }
            Message::FinishSidebarResize => {
                let Some((_, _, sidebar_width)) = self.sidebar_resize.take() else {
                    return Task::none();
                };
                let mut browser = self.active_browser_settings();
                browser.sidebar_width = sidebar_width;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::ShowEntryContext { path, is_directory } => {
                self.selected_entries.clear();
                self.selected_entries.insert(path.clone());
                self.selection_anchor = Some(path.clone());
                self.context_entry = Some(ContextEntry {
                    path: path.clone(),
                    is_directory,
                    is_sidebar_location: false,
                    opener: None,
                    open_with_expanded: false,
                    open_with_apps: None,
                });
                self.context_position = self.pointer_position;
                if is_directory {
                    Task::none()
                } else {
                    Task::perform(default_file_opener(path.clone()), move |opener| {
                        Message::FileOpenerResolved {
                            path: path.clone(),
                            opener,
                        }
                    })
                }
            }
            Message::ShowSidebarLocationContext(path) => {
                self.context_entry = Some(ContextEntry {
                    path,
                    is_directory: true,
                    is_sidebar_location: true,
                    opener: None,
                    open_with_expanded: false,
                    open_with_apps: None,
                });
                self.context_position = self.pointer_position;
                Task::none()
            }
            Message::SetSidebarLocationIcon { path, icon } => {
                self.set_sidebar_location_icon(path, icon);
                Task::none()
            }
            Message::FileOpenerResolved { path, opener } => {
                if let Some(context_entry) = &mut self.context_entry
                    && !context_entry.is_directory
                    && context_entry.path == path
                {
                    context_entry.opener = Some(opener);
                }
                Task::none()
            }
            Message::ContextPointerMoved(position) => {
                self.pointer_position = position;
                if let Some((start_x, initial_width, _)) = self.sidebar_resize {
                    let sidebar_width = (f32::from(initial_width) + position.x - start_x)
                        .round()
                        .clamp(140.0, 600.0) as u16;
                    self.sidebar_resize = Some((start_x, initial_width, sidebar_width));
                }
                Task::none()
            }
            Message::CloseFolderContext => {
                self.context_entry = None;
                Task::none()
            }
            Message::TogglePerformanceDebugger => {
                self.show_performance_debugger = !self.show_performance_debugger;
                Task::none()
            }
            Message::CopyPerformanceReport => {
                let Some(load) = &self.folder_load_performance else {
                    self.status = "No folder-load performance data to copy".into();
                    return Task::none();
                };
                let displayed_ms = load
                    .displayed_at
                    .map(|at| at.duration_since(load.started_at).as_millis())
                    .unwrap_or_default();
                let thumbnails_ms = load
                    .thumbnails_settled_at
                    .map(|at| at.duration_since(load.started_at).as_millis())
                    .unwrap_or_default();
                let item_milestones = [10, 25, 50, 75, 90]
                    .into_iter()
                    .filter_map(|percent| {
                        let index = load
                            .expected_entries
                            .saturating_mul(percent)
                            .div_ceil(100)
                            .saturating_sub(1);
                        load.item_rendered_at.get(index).map(|at| {
                            format!(
                                "  {percent}%: {} ms",
                                at.duration_since(load.started_at).as_millis()
                            )
                        })
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut item_intervals = load
                    .item_rendered_at
                    .windows(2)
                    .map(|timestamps| {
                        timestamps[1].duration_since(timestamps[0]).as_secs_f64() * 1_000.0
                    })
                    .collect::<Vec<_>>();
                item_intervals.sort_by(f64::total_cmp);
                let item_render_stats = if item_intervals.is_empty() {
                    "Item render intervals: unavailable".to_owned()
                } else {
                    let average = item_intervals.iter().sum::<f64>() / item_intervals.len() as f64;
                    let percentile =
                        |percent: usize| item_intervals[(item_intervals.len() - 1) * percent / 100];
                    format!(
                        "Item render interval: avg {average:.2} ms | min {:.2} ms | p5 {:.2} ms | p95 {:.2} ms | max {:.2} ms",
                        item_intervals[0],
                        percentile(5),
                        percentile(95),
                        item_intervals[item_intervals.len() - 1]
                    )
                };
                let entry_count = load.item_rendered_at.len().max(1) as f64;
                let backend_item_averages = format!(
                    "Average backend item steps: path {:.3} ms | symlink {:.3} ms | metadata {:.3} ms | timestamps {:.3} ms",
                    load.entry_path_us as f64 / entry_count / 1_000.0,
                    load.entry_symlink_us as f64 / entry_count / 1_000.0,
                    load.entry_metadata_us as f64 / entry_count / 1_000.0,
                    load.entry_timestamps_us as f64 / entry_count / 1_000.0,
                );
                self.status = "Copied performance report to the clipboard".into();
                iced::clipboard::write(format!(
                    "Folder load: {}\nBrowse response: {} ms\nBackend enumeration: {} ms\nFirst item rendered: {} ms\nAll items displayed: {} ms\nAll thumbnails loaded: {} ms ({}/{})\nEntries: {}\n\nItem render milestones:\n{}\n\n{}\n{}",
                    self.directory_path.display(),
                    self.last_folder_load_duration
                        .map(|duration| duration.as_millis())
                        .unwrap_or_default(),
                    load.enumeration_ms.unwrap_or_default(),
                    load.first_item_at
                        .map(|at| at.duration_since(load.started_at).as_millis())
                        .unwrap_or_default(),
                    displayed_ms,
                    thumbnails_ms,
                    load.thumbnails_settled,
                    load.thumbnails_total,
                    load.expected_entries,
                    item_milestones,
                    item_render_stats,
                    backend_item_averages,
                ))
            }
            Message::RequestEntryInfo(path) => {
                self.context_entry = None;
                self.pending_info = Some(InfoDialog::Loading(path.clone()));
                Task::perform(inspect_entry(path.clone()), move |result| {
                    Message::EntryInfoLoaded {
                        path: path.clone(),
                        result,
                    }
                })
            }
            Message::EntryInfoLoaded { path, result } => {
                if !matches!(self.pending_info, Some(InfoDialog::Loading(ref current)) if current == &path)
                {
                    return Task::none();
                }
                self.pending_info = Some(match result {
                    Ok(info) => InfoDialog::Loaded(EntryInfo {
                        path: path.clone(),
                        name: info.name,
                        rows: info
                            .fields
                            .into_iter()
                            .map(|field| (field.label, field.value))
                            .collect(),
                    }),
                    Err(error) => InfoDialog::Error { path, error },
                });
                Task::none()
            }
            Message::CloseEntryInfo => {
                self.pending_info = None;
                Task::none()
            }
            Message::OpenContextFile => {
                let Some(context_entry) = self.context_entry.take() else {
                    return Task::none();
                };
                Task::perform(open_file(context_entry.path), Message::FileOpened)
            }
            Message::ToggleOpenWith => {
                let Some(context_entry) = &mut self.context_entry else {
                    return Task::none();
                };
                context_entry.open_with_expanded = !context_entry.open_with_expanded;
                if context_entry.open_with_expanded && context_entry.open_with_apps.is_none() {
                    let path = context_entry.path.clone();
                    Task::perform(list_open_with_apps(path.clone()), move |apps| {
                        Message::OpenWithAppsLoaded {
                            path: path.clone(),
                            apps,
                        }
                    })
                } else {
                    Task::none()
                }
            }
            Message::OpenWithAppsLoaded { path, apps } => {
                if let Some(context_entry) = &mut self.context_entry
                    && context_entry.path == path
                {
                    context_entry.open_with_apps = Some(apps);
                }
                Task::none()
            }
            Message::OpenWithApp(app_id) => {
                let Some(context_entry) = self.context_entry.take() else {
                    return Task::none();
                };
                Task::perform(
                    open_with_app(context_entry.path, app_id),
                    Message::FileOpened,
                )
            }
            Message::SetDefaultApp(app_id) => {
                let Some(context_entry) = &self.context_entry else {
                    return Task::none();
                };
                let path = context_entry.path.clone();
                Task::perform(
                    set_default_app_for_path(path.clone(), app_id),
                    move |result| Message::DefaultAppSet {
                        path: path.clone(),
                        result,
                    },
                )
            }
            Message::DefaultAppSet { path, result } => {
                self.status = match &result {
                    Ok(()) => "Default application updated".into(),
                    Err(error) => error.clone(),
                };
                let Some(context_entry) = &mut self.context_entry else {
                    return Task::none();
                };
                if context_entry.path != path {
                    return Task::none();
                }
                context_entry.open_with_apps = None;
                context_entry.opener = None;
                Task::perform(default_file_opener(path.clone()), move |opener| {
                    Message::FileOpenerResolved {
                        path: path.clone(),
                        opener,
                    }
                })
            }
            Message::OpenTerminalHere => {
                let Some(ContextEntry {
                    path,
                    is_directory: true,
                    ..
                }) = self.context_entry.take()
                else {
                    return Task::none();
                };
                let command = self.active_browser_settings().terminal_command;
                Task::perform(open_terminal(path, command), Message::TerminalOpened)
            }
            Message::AddContextFolderToSidebar => {
                self.add_context_folder_to_sidebar();
                Task::none()
            }
            Message::RemoveContextFolderFromSidebar => {
                self.remove_context_folder_from_sidebar();
                Task::none()
            }
            Message::SidebarPressed(path) => {
                self.sidebar_resize = None;
                self.dragging_sidebar_location = Some(path);
                self.sidebar_drop_target = None;
                self.sidebar_drop_at_end = false;
                Task::none()
            }
            Message::SidebarReleased(path) => {
                if self.dragging_entries.is_some() {
                    self.sidebar_drop_target = None;
                    self.drop_dragged_entries(path)
                } else {
                    self.release_sidebar_location(path)
                }
            }
            Message::SidebarDragTarget(path) => {
                if self.dragging_sidebar_location.is_some() || self.dragging_entries.is_some() {
                    self.sidebar_drop_target = Some(path);
                }
                Task::none()
            }
            Message::SidebarDragTargetCleared(path) => {
                if self.sidebar_drop_target.as_ref() == Some(&path) {
                    self.sidebar_drop_target = None;
                }
                Task::none()
            }
            Message::SidebarDragTargetEnd => {
                if self.dragging_sidebar_location.is_some() {
                    self.sidebar_drop_target = None;
                    self.sidebar_drop_at_end = true;
                }
                Task::none()
            }
            Message::SidebarDragTargetEndCleared => {
                self.sidebar_drop_at_end = false;
                Task::none()
            }
            Message::SidebarReleasedAtEnd => self.release_sidebar_location_at_end(),
            Message::MountsLoaded(result) => {
                match result {
                    Ok(state) => {
                        self.drives = state.drives;
                        self.mounts = state.mounts;
                    }
                    Err(error) => self.status = error,
                }
                Task::none()
            }
            Message::MountDrive(path) => {
                self.status = format!("Mounting {}", path.display());
                Task::perform(mount_drive(path), Message::MountsLoaded)
            }
            Message::FileOpened(result) => {
                self.status = match result {
                    Ok(()) => "Opened file".into(),
                    Err(error) => error,
                };
                Task::none()
            }
            Message::TerminalOpened(result) => {
                self.status = match result {
                    Ok(()) => "Opened terminal".into(),
                    Err(error) => error,
                };
                Task::none()
            }
            Message::BackendLogPipeEnded(Err(error)) => {
                self.status = format!("Backend log stream stopped: {error}");
                Task::none()
            }
            Message::BackendLogPipeEnded(Ok(())) => Task::none(),
            Message::RestartBackend => {
                self.status = "Restarting backend".into();
                Task::perform(restart_backend(), Message::BackendRestarted)
            }
            Message::BackendRestarted(result) => {
                self.status = match result {
                    Ok(()) => "Backend restarted".into(),
                    Err(error) => format!("Could not restart backend: {error}"),
                };
                Task::none()
            }
            Message::ThumbnailGenerated {
                path,
                thumbnail_path: Ok(thumbnail_path),
            } => {
                if let Some(performance) = &mut self.folder_load_performance {
                    performance.thumbnails_settled += 1;
                    performance.thumbnail_settled_at.push(Instant::now());
                    if performance.displayed_at.is_some()
                        && performance.thumbnails_settled == performance.thumbnails_total
                    {
                        performance.thumbnails_settled_at = Some(Instant::now());
                    }
                }
                if !thumbnail_path.is_empty()
                    && let Some(entry) = self
                        .entries
                        .iter_mut()
                        .find(|entry| PathBuf::from(&entry.path) == path)
                {
                    entry.thumbnail_path = thumbnail_path;
                }
                if let Some(entry) = self
                    .entries
                    .iter()
                    .find(|entry| PathBuf::from(&entry.path) == path)
                    && let Ok(bytes) = fs::read(&entry.thumbnail_path)
                {
                    self.thumbnail_handles
                        .insert(path, image::Handle::from_bytes(bytes));
                }
                Task::none()
            }
            Message::ThumbnailGenerated {
                thumbnail_path: Err(error),
                ..
            } => {
                if let Some(performance) = &mut self.folder_load_performance {
                    performance.thumbnails_settled += 1;
                    performance.thumbnail_settled_at.push(Instant::now());
                }
                eprintln!("[iron-file thumbnails] {error}");
                Task::none()
            }
            Message::DirectoryEntriesLoaded { directory, entries } => match entries {
                Ok(entries) if self.directory_path == directory => {
                    let icon_theme = self.active_browser_settings().icon_theme;
                    let icon_names = gio_icon_names_batch(
                        &entries
                            .iter()
                            .filter(|entry| !entry.directory_complete)
                            .map(|entry| PathBuf::from(&entry.path))
                            .collect::<Vec<_>>(),
                    );
                    let mut completed = None;
                    for entry in entries {
                        if entry.directory_complete {
                            completed = Some(entry);
                            continue;
                        }
                        let path = PathBuf::from(&entry.path);
                        let now = Instant::now();
                        if let Some(performance) = &mut self.folder_load_performance {
                            performance.first_item_at.get_or_insert(now);
                            performance.item_rendered_at.push(now);
                            performance.entry_path_us += entry.entry_path_ms;
                            performance.entry_symlink_us += entry.entry_symlink_ms;
                            performance.entry_metadata_us += entry.entry_metadata_ms;
                            performance.entry_timestamps_us += entry.entry_timestamps_ms;
                        }
                        let icon = self.cached_entry_icon_path_with_names(
                            &icon_theme,
                            &entry,
                            icon_names.get(&path),
                        );
                        self.entry_icons.insert(path.clone(), icon);
                        if !entry.is_directory && !self.thumbnail_handles.contains_key(&path) {
                            if let Some(performance) = &mut self.folder_load_performance {
                                performance.thumbnails_total += 1;
                            }
                            self.pending_thumbnail_paths.push(path);
                        }
                        self.entries.push(entry);
                    }
                    let sort_order = self.current_sort_order();
                    sort_entries(&mut self.entries, sort_order);
                    self.status = format!("{} entries", self.entries.len());
                    if let Some(entry) = completed {
                        if let Some(performance) = &mut self.folder_load_performance {
                            performance.enumeration_ms = Some(entry.directory_enumeration_ms);
                            performance.expected_entries = entry.directory_entry_count as usize;
                            performance.displayed_at = Some(Instant::now());
                        }
                        let thumbnail_directory = self.active_browser_settings().thumbnail_location;
                        return Task::batch(self.pending_thumbnail_paths.drain(..).map(|path| {
                            Task::perform(
                                create_thumbnail(path.clone(), thumbnail_directory.clone()),
                                move |thumbnail_path| Message::ThumbnailGenerated {
                                    path: path.clone(),
                                    thumbnail_path,
                                },
                            )
                        }));
                    }
                    Task::none()
                }
                Ok(_) => Task::none(),
                Err(error) => self.update(Message::DirectoryEntryLoaded {
                    directory,
                    entry: Err(error),
                }),
            },
            Message::DirectoryEntryLoaded {
                directory,
                entry: Ok(entry),
            } => {
                if self.directory_path != directory {
                    return Task::none();
                }
                if entry.directory_complete {
                    if let Some(performance) = &mut self.folder_load_performance {
                        performance.enumeration_ms = Some(entry.directory_enumeration_ms);
                        performance.expected_entries = entry.directory_entry_count as usize;
                        performance.displayed_at = Some(Instant::now());
                        if performance.thumbnails_total == performance.thumbnails_settled {
                            performance.thumbnails_settled_at = Some(Instant::now());
                        }
                    }
                    let thumbnail_directory = self.active_browser_settings().thumbnail_location;
                    return Task::batch(self.pending_thumbnail_paths.drain(..).map(|path| {
                        Task::perform(
                            create_thumbnail(path.clone(), thumbnail_directory.clone()),
                            move |thumbnail_path| Message::ThumbnailGenerated {
                                path: path.clone(),
                                thumbnail_path,
                            },
                        )
                    }));
                }
                let path = PathBuf::from(&entry.path);
                if let Some(performance) = &mut self.folder_load_performance
                    && performance.first_item_at.is_none()
                {
                    performance.first_item_at = Some(Instant::now());
                }
                if let Some(performance) = &mut self.folder_load_performance {
                    performance.item_rendered_at.push(Instant::now());
                    performance.entry_path_us += entry.entry_path_ms;
                    performance.entry_symlink_us += entry.entry_symlink_ms;
                    performance.entry_metadata_us += entry.entry_metadata_ms;
                    performance.entry_timestamps_us += entry.entry_timestamps_ms;
                }
                let icon_theme = self.active_browser_settings().icon_theme;
                let icon = self.cached_entry_icon_path(&icon_theme, &entry);
                self.entry_icons.insert(path.clone(), icon);
                let is_directory = entry.is_directory;
                let thumbnail_is_loaded = self.thumbnail_handles.contains_key(&path);
                let sort_order = self.current_sort_order();
                self.entries.push(entry);
                sort_entries(&mut self.entries, sort_order);
                self.status = format!("{} entries", self.entries.len());
                if is_directory || thumbnail_is_loaded {
                    Task::none()
                } else {
                    if let Some(performance) = &mut self.folder_load_performance {
                        performance.thumbnails_total += 1;
                    }
                    self.pending_thumbnail_paths.push(path);
                    Task::none()
                }
            }
            Message::DirectoryEntryLoaded {
                directory,
                entry: Err(error),
            } => {
                if self.directory_path == directory {
                    self.status = format!("Could not load folder contents: {error}");
                }
                Task::none()
            }
            Message::BrowseFinished { result, history } => self.apply_response(result, history),
            Message::IconFontLoaded(_) => Task::none(),
        }
    }

    fn theme(&self) -> Theme {
        let base = match self.color_mode {
            ColorMode::Day => Theme::Light,
            ColorMode::Night => Theme::Dark,
            ColorMode::System => Theme::Dark,
        };
        let theme_settings = self.active_theme_settings();
        let highlight = if matches!(base, Theme::Dark) {
            &theme_settings.dark_highlight
        } else {
            &self.active_theme_settings().light_highlight
        };
        let Some(highlight) = parse_color(highlight) else {
            return base;
        };
        let mut palette = base.palette();
        palette.primary = highlight;
        palette.background = palette
            .background
            .scale_alpha(f32::from(theme_settings.background_opacity.min(100)) / 100.0);
        Theme::custom("Iron File", palette)
    }

    fn active_theme_settings(&self) -> iron_file_common::config::ThemeSettings {
        self.active_profile
            .as_deref()
            .and_then(|path| self.profiles.iter().find(|profile| profile.path == path))
            .map(|profile| profile.theme.clone())
            .unwrap_or_else(iron_file_common::config::default_theme_settings)
    }

    fn accent_picker_button(&self, dark: bool) -> Element<'_, Message> {
        let color = parse_color(if dark {
            &self.dark_accent_input
        } else {
            &self.light_accent_input
        })
        .unwrap_or(Color::BLACK);
        button(
            row![
                container(
                    Space::new()
                        .width(Length::Fixed(20.0))
                        .height(Length::Fixed(20.0))
                )
                .style(move |_| iced::widget::container::Style::default().background(color)),
                text(if dark {
                    "Dark accent color"
                } else {
                    "Light accent color"
                }),
            ]
            .spacing(8),
        )
        .on_press(Message::OpenAccentPicker(dark))
        .into()
    }

    fn preference_reset_button(&self, option: PreferenceOption) -> Element<'_, Message> {
        if self.preference_matches_default(option) {
            return Space::with_width(Length::Fixed(0.0)).into();
        }
        tooltip(
            button(icon_text("rotate-ccw").size(16)).on_press(Message::ResetPreference(option)),
            text("Reset to default"),
            tooltip::Position::Bottom,
        )
        .into()
    }

    fn preference_matches_default(&self, option: PreferenceOption) -> bool {
        let browser = self.active_browser_settings();
        let browser_defaults = iron_file_common::config::default_browser_settings();
        let default_thumbnail_location = browser_defaults
            .thumbnail_location
            .to_str()
            .and_then(|path| path.strip_prefix("~/"))
            .and_then(|path| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(path)))
            .unwrap_or_else(|| browser_defaults.thumbnail_location.clone());
        let theme = self.active_theme_settings();
        let theme_defaults = iron_file_common::config::default_theme_settings();
        match option {
            PreferenceOption::ColorMode => self.color_mode == ColorMode::default(),
            PreferenceOption::LightAccent => {
                theme.light_highlight == theme_defaults.light_highlight
            }
            PreferenceOption::DarkAccent => theme.dark_highlight == theme_defaults.dark_highlight,
            PreferenceOption::BackgroundOpacity => {
                theme.background_opacity == theme_defaults.background_opacity
            }
            PreferenceOption::ContextMenuBlurStrength => {
                theme.context_menu_blur_strength == theme_defaults.context_menu_blur_strength
            }
            PreferenceOption::ContextMenuBlurKernelSize => {
                theme.context_menu_blur_kernel_size == theme_defaults.context_menu_blur_kernel_size
            }
            PreferenceOption::BorderRadius => theme.border_radius == theme_defaults.border_radius,
            PreferenceOption::Layout => browser.layout == browser_defaults.layout,
            PreferenceOption::NameAlignment => {
                browser.name_alignment == browser_defaults.name_alignment
            }
            PreferenceOption::SmoothScrolling => {
                browser.smooth_scrolling == browser_defaults.smooth_scrolling
            }
            PreferenceOption::ScrollStep => browser.scroll_step == browser_defaults.scroll_step,
            PreferenceOption::ItemSize => browser.item_size == browser_defaults.item_size,
            PreferenceOption::MaxNameLines => {
                browser.max_name_lines == browser_defaults.max_name_lines
            }
            PreferenceOption::Preview => {
                browser.preview_enabled == browser_defaults.preview_enabled
            }
            PreferenceOption::SingleClickFolders => {
                browser.single_click_opens_folders == browser_defaults.single_click_opens_folders
            }
            PreferenceOption::IconTheme => browser.icon_theme == browser_defaults.icon_theme,
            PreferenceOption::ThumbnailLocation => {
                browser.thumbnail_location == default_thumbnail_location
            }
            PreferenceOption::Terminal => {
                browser.terminal_command == browser_defaults.terminal_command
            }
            PreferenceOption::FileContextMenuItems => {
                browser.file_context_menu_items == browser_defaults.file_context_menu_items
            }
            PreferenceOption::FolderContextMenuItems => {
                browser.folder_context_menu_items == browser_defaults.folder_context_menu_items
            }
            PreferenceOption::QuickToolbarItems => {
                browser.quick_toolbar_items == browser_defaults.quick_toolbar_items
            }
            PreferenceOption::KeyboardShortcuts => {
                browser.keyboard_shortcuts == browser_defaults.keyboard_shortcuts
            }
        }
    }

    fn active_browser_settings(&self) -> BrowserSettings {
        self.active_profile
            .as_deref()
            .and_then(|path| self.profiles.iter().find(|profile| profile.path == path))
            .map(|profile| profile.browser.clone())
            .unwrap_or_else(iron_file_common::config::default_browser_settings)
    }

    fn folder_sort_override(&self, path: &Path) -> Option<EntrySortOrder> {
        self.active_browser_settings()
            .folder_sort_overrides
            .into_iter()
            .find(|override_| override_.path == path)
            .map(|override_| override_.sort_order)
    }

    fn current_sort_order(&self) -> EntrySortOrder {
        self.folder_sort_override(&self.directory_path)
            .unwrap_or_else(|| self.active_browser_settings().sort_order)
    }

    fn sidebar_width(&self) -> u16 {
        self.sidebar_resize
            .map(|(_, _, width)| width)
            .unwrap_or_else(|| self.active_browser_settings().sidebar_width)
    }

    fn terminal_choices(&self) -> Vec<String> {
        let mut choices = vec![DEFAULT_TERMINAL_CHOICE.into()];
        choices.extend(self.terminal_recommendations.clone());
        choices.push(CUSTOM_TERMINAL_CHOICE.into());
        choices
    }

    fn selected_terminal_choice(&self, browser: &BrowserSettings) -> String {
        if browser.terminal_command == "default" {
            DEFAULT_TERMINAL_CHOICE.into()
        } else if self
            .terminal_recommendations
            .contains(&browser.terminal_command)
        {
            browser.terminal_command.clone()
        } else {
            CUSTOM_TERMINAL_CHOICE.into()
        }
    }

    fn icon_theme_choices(&self, browser: &BrowserSettings) -> Vec<String> {
        let mut themes = self.icon_themes.clone();
        if !themes.contains(&browser.icon_theme) {
            themes.push(browser.icon_theme.clone());
        }
        themes
    }

    fn refresh_entry_icons(&mut self) {
        let icon_theme = self.active_browser_settings().icon_theme;
        self.entry_icon_cache.clear();
        let entries = self.entries.clone();
        self.entry_icons = entries
            .iter()
            .map(|entry| {
                (
                    PathBuf::from(&entry.path),
                    self.cached_entry_icon_path(&icon_theme, entry),
                )
            })
            .collect();
    }

    fn cached_entry_icon_path(&mut self, theme: &str, entry: &proto::FileEntry) -> Option<PathBuf> {
        self.cached_entry_icon_path_with_names(theme, entry, None)
    }

    fn cached_entry_icon_path_with_names(
        &mut self,
        theme: &str,
        entry: &proto::FileEntry,
        icon_names: Option<&Vec<String>>,
    ) -> Option<PathBuf> {
        let key = (
            theme.to_owned(),
            entry.is_directory,
            (!entry.is_directory).then(|| {
                Path::new(&entry.path)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase()
            }),
        );
        self.entry_icon_cache
            .entry(key)
            .or_insert_with(|| {
                icon_names.map_or_else(
                    || themed_entry_icon_path(theme, entry),
                    |icons| themed_icon_path(theme, icons),
                )
            })
            .clone()
    }

    fn handle_entry_click(&mut self, path: PathBuf, is_directory: bool) -> Task<Message> {
        if let Some(picker) = self.picker.as_ref() {
            let now = Instant::now();
            let is_double_click =
                self.last_entry_click
                    .as_ref()
                    .is_some_and(|(last_path, last_click)| {
                        last_path == &path
                            && now.duration_since(*last_click) <= Duration::from_millis(500)
                    });
            self.last_entry_click = Some((path.clone(), now));
            if is_directory && is_double_click {
                self.last_entry_click = None;
                return self.open_path(path);
            }
            if (picker.kind == PickerKind::Folder) == is_directory {
                self.select_entry(&path);
            }
            return Task::none();
        }
        self.select_entry(&path);
        let now = Instant::now();
        let is_double_click =
            self.last_entry_click
                .as_ref()
                .is_some_and(|(last_path, last_click)| {
                    last_path == &path
                        && now.duration_since(*last_click) <= Duration::from_millis(500)
                });
        self.last_entry_click = Some((path.clone(), now));

        if is_directory {
            if self.active_browser_settings().single_click_opens_folders || is_double_click {
                self.last_entry_click = None;
                self.open_path(path)
            } else {
                Task::none()
            }
        } else if is_double_click {
            self.last_entry_click = None;
            Task::perform(open_file(path), Message::FileOpened)
        } else {
            self.open_path(path)
        }
    }

    fn select_entry(&mut self, path: &Path) {
        if self.picker.as_ref().is_some_and(|picker| !picker.multiple) {
            self.selected_entries.clear();
            self.selected_entries.insert(path.to_path_buf());
            self.selection_anchor = Some(path.to_path_buf());
            return;
        }
        let add_to_selection = self.modifiers.command();
        let range_selection = self.modifiers.shift();
        if range_selection {
            let anchor = self.selection_anchor.as_deref().unwrap_or(path);
            let anchor_index = self
                .entries
                .iter()
                .position(|entry| Path::new(&entry.path) == anchor);
            let target_index = self
                .entries
                .iter()
                .position(|entry| Path::new(&entry.path) == path);
            if let (Some(anchor_index), Some(target_index)) = (anchor_index, target_index) {
                if !add_to_selection {
                    self.selected_entries.clear();
                }
                let (start, end) = if anchor_index <= target_index {
                    (anchor_index, target_index)
                } else {
                    (target_index, anchor_index)
                };
                self.selected_entries.extend(
                    self.entries[start..=end]
                        .iter()
                        .map(|entry| PathBuf::from(&entry.path)),
                );
            }
        } else if add_to_selection {
            if !self.selected_entries.insert(path.to_path_buf()) {
                self.selected_entries.remove(path);
                if self.selection_anchor.as_deref() == Some(path) {
                    self.selection_anchor = None;
                }
            } else {
                self.selection_anchor = Some(path.to_path_buf());
            }
        } else {
            self.selected_entries.clear();
            self.selected_entries.insert(path.to_path_buf());
            self.selection_anchor = Some(path.to_path_buf());
        }
        if range_selection {
            self.selection_anchor = Some(path.to_path_buf());
        }
    }

    fn move_selected_entry(&mut self, direction: SelectionDirection) {
        if self.view != View::Browser
            || self.editing_address
            || self.pending_create.is_some()
            || self.pending_rename.is_some()
            || self.pending_compression
            || self.selected_entries.len() != 1
        {
            return;
        }
        let browser = self.active_browser_settings();
        let entries = self
            .entries
            .iter()
            .filter(|entry| browser.show_hidden_files || !entry.name.starts_with('.'))
            .collect::<Vec<_>>();
        let Some(index) = entries
            .iter()
            .position(|entry| self.selected_entries.contains(Path::new(&entry.path)))
        else {
            return;
        };
        let target = match (browser.layout, direction) {
            (_, SelectionDirection::Left | SelectionDirection::Up)
                if browser.layout == BrowserLayout::List =>
            {
                index.checked_sub(1)
            }
            (_, SelectionDirection::Right | SelectionDirection::Down)
                if browser.layout == BrowserLayout::List =>
            {
                (index + 1 < entries.len()).then_some(index + 1)
            }
            (_, SelectionDirection::Left) => index.checked_sub(1),
            (_, SelectionDirection::Right) => (index + 1 < entries.len()).then_some(index + 1),
            (_, SelectionDirection::Up) => index.checked_sub(self.tile_columns.get().max(1)),
            (_, SelectionDirection::Down) => (index + self.tile_columns.get().max(1)
                < entries.len())
            .then_some(index + self.tile_columns.get().max(1)),
        };
        let Some(target) = target else {
            return;
        };
        let path = PathBuf::from(&entries[target].path);
        self.selected_entries.clear();
        self.selected_entries.insert(path.clone());
        self.selection_anchor = Some(path);
        self.last_entry_click = None;
    }

    fn confirm_picker(&mut self) -> Task<Message> {
        let Some(picker) = self.picker.as_ref() else {
            return Task::none();
        };
        if self
            .save_file_name
            .as_deref()
            .is_some_and(|name| !valid_save_file_name(name))
        {
            self.status = "Enter a simple file name".into();
            return Task::none();
        }
        let mut selected = self
            .selected_entries
            .iter()
            .filter(|path| {
                std::fs::metadata(path)
                    .map(|metadata| (picker.kind == PickerKind::Folder) == metadata.is_dir())
                    .unwrap_or(false)
            })
            .cloned()
            .collect::<Vec<_>>();
        selected.sort();
        if selected.is_empty() {
            if picker.kind == PickerKind::Folder {
                selected.push(self.directory_path.clone());
            } else {
                self.status = "Select a file first".into();
                return Task::none();
            }
        }
        if !picker.multiple {
            selected.truncate(1);
        }
        for path in selected {
            let path = self
                .save_file_name
                .as_deref()
                .map(|name| path.join(name))
                .unwrap_or(path);
            println!("{}", path.display());
        }
        self.close_window()
    }

    fn close_window(&self) -> Task<Message> {
        if self.picker.is_some() {
            let _ = self
                .config_store
                .set_last_picker_directory(&self.directory_path);
        }
        iced::exit()
    }

    fn execute_browser_command(&mut self, command: BrowserCommand) -> Task<Message> {
        if self.view != View::Browser || self.editing_address {
            return Task::none();
        }
        match command {
            BrowserCommand::CopySelection => {
                let mut entries = self.selected_entries.iter().cloned().collect::<Vec<_>>();
                entries.sort();
                if entries.is_empty() {
                    self.status = "Select files or folders to copy".into();
                } else {
                    self.paste_buffer = Some(PasteBuffer {
                        entries,
                        mode: PasteMode::Copy,
                    });
                    self.context_entry = None;
                    let count = self
                        .paste_buffer
                        .as_ref()
                        .map_or(0, |buffer| buffer.entries.len());
                    self.status = format!("Copied {count} item(s) to the clipboard");
                }
                Task::none()
            }
            BrowserCommand::CutSelection => {
                let mut entries = self.selected_entries.iter().cloned().collect::<Vec<_>>();
                entries.sort();
                if entries.is_empty() {
                    self.status = "Select files or folders to cut".into();
                } else {
                    let count = entries.len();
                    self.paste_buffer = Some(PasteBuffer {
                        entries,
                        mode: PasteMode::Move,
                    });
                    self.context_entry = None;
                    self.status = format!("Cut {count} item(s) to the clipboard");
                }
                Task::none()
            }
            BrowserCommand::CancelCut => {
                if matches!(
                    self.paste_buffer.as_ref().map(|buffer| buffer.mode),
                    Some(PasteMode::Move)
                ) {
                    self.paste_buffer = None;
                    self.status = "Cut cancelled".into();
                }
                Task::none()
            }
            BrowserCommand::RenameSelection => {
                if self.selected_entries.len() != 1 {
                    self.status = "Select exactly one file or folder to rename".into();
                    return Task::none();
                }
                let path = self
                    .selected_entries
                    .iter()
                    .next()
                    .cloned()
                    .unwrap_or_default();
                self.update(Message::RequestRenameEntry(path))
            }
            BrowserCommand::CopyLocation(path) => {
                self.context_entry = None;
                self.status = "Copied location to the clipboard".into();
                iced::clipboard::write(path.to_string_lossy().into_owned())
            }
            BrowserCommand::Paste => {
                let Some(buffer) = self.paste_buffer.clone() else {
                    self.status = "Nothing to paste".into();
                    return Task::none();
                };
                self.context_entry = None;
                self.status = format!("Pasting {} item(s)...", buffer.entries.len());
                match buffer.mode {
                    PasteMode::Copy => Task::perform(
                        copy_entries(buffer.entries, self.directory_path.clone()),
                        Message::FileCopyFinished,
                    ),
                    PasteMode::Move => {
                        let destination = self.directory_path.clone();
                        let sources = buffer
                            .entries
                            .into_iter()
                            .filter(|path| path.parent() != Some(destination.as_path()))
                            .collect::<Vec<_>>();
                        if sources.is_empty() {
                            self.paste_buffer = None;
                            self.status = "Item(s) are already in this folder".into();
                            return Task::none();
                        }
                        let to_delete = sources.clone();
                        Task::perform(copy_entries(sources, destination), move |result| {
                            Message::CutPasteFinished {
                                sources: to_delete.clone(),
                                result,
                            }
                        })
                    }
                    PasteMode::Symlink => Task::perform(
                        create_symlinks(buffer.entries, self.directory_path.clone()),
                        Message::FileCopyFinished,
                    ),
                }
            }
            BrowserCommand::DeleteSelection => {
                let mut paths = self.selected_entries.iter().cloned().collect::<Vec<_>>();
                paths.sort();
                if paths.is_empty() {
                    self.status = "Select files or folders to delete".into();
                } else {
                    self.context_entry = None;
                    self.delete_confirm_selected = false;
                    self.pending_delete = Some(paths);
                }
                Task::none()
            }
            BrowserCommand::AddSymlinkToPasteBuffer(path) => {
                self.paste_buffer = Some(PasteBuffer {
                    entries: vec![path],
                    mode: PasteMode::Symlink,
                });
                self.context_entry = None;
                self.status = "Added symbolic link to the paste buffer".into();
                Task::none()
            }
            BrowserCommand::CreateSymlinksHere(source) => {
                self.context_entry = None;
                self.status = "Creating symbolic link...".into();
                Task::perform(
                    create_symlinks(vec![source], self.directory_path.clone()),
                    Message::FileCopyFinished,
                )
            }
            BrowserCommand::CompressSelection => {
                if self.selected_entries.is_empty() {
                    self.status = "Select files or folders first".into();
                } else {
                    self.pending_compression = true;
                }
                Task::none()
            }
            BrowserCommand::ExtractSelection => {
                let mut paths = self.selected_entries.iter().cloned().collect::<Vec<_>>();
                paths.sort();
                if paths.is_empty() {
                    self.status = "Select files or folders first".into();
                    return Task::none();
                }
                self.status = format!("Extracting {} archive(s)...", paths.len());
                Task::perform(
                    extract_archives(paths, self.directory_path.clone()),
                    |result| Message::ArchiveFinished {
                        action: "Extracted",
                        result,
                    },
                )
            }
        }
    }

    fn execute_compression(&mut self) -> Task<Message> {
        let mut paths = self.selected_entries.iter().cloned().collect::<Vec<_>>();
        paths.sort();
        if paths.is_empty() {
            self.status = "Select files or folders first".into();
            return Task::none();
        }
        self.status = format!("Compressing {} item(s)...", paths.len());
        Task::perform(
            compress_entries(
                paths,
                self.directory_path.clone(),
                i32::from(self.compression_level),
                self.compression_type.value().into(),
            ),
            |result| Message::ArchiveFinished {
                action: "Compressed",
                result,
            },
        )
    }

    fn update_rectangle_selection(&mut self) {
        let Some(selection) = self.rectangle_selection.clone() else {
            return;
        };

        let offset = self.browser_scroll_offset;
        let left = selection.start.x.min(selection.end.x) + offset.x;
        let right = selection.start.x.max(selection.end.x) + offset.x;
        let top = selection.start.y.min(selection.end.y) + offset.y;
        let bottom = selection.start.y.max(selection.end.y) + offset.y;
        let browser = self.active_browser_settings();
        let tile_width = f32::from(browser.item_size) * 3.5;
        let tile_height = tile_width * 1.2;
        let row_height = f32::from(browser.item_size).max(24.0) + 12.0;
        let columns = self.tile_columns.get().max(1);

        self.selected_entries = selection.initial_selection;
        for (index, entry) in self
            .entries
            .iter()
            .filter(|entry| browser.show_hidden_files || !entry.name.starts_with('.'))
            .enumerate()
        {
            let (x, y, width, height) = if browser.layout == BrowserLayout::Tiles {
                let column = index % columns;
                let row = index / columns;
                (
                    column as f32 * (tile_width + 8.0),
                    row as f32 * (tile_height + 8.0),
                    tile_width,
                    tile_height,
                )
            } else {
                (0.0, index as f32 * row_height, f32::INFINITY, row_height)
            };
            if x < right && x + width > left && y < bottom && y + height > top {
                self.selected_entries.insert(PathBuf::from(&entry.path));
            }
        }
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
