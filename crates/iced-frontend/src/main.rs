use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};

use iced::{
    Background, Border, Color, Element, Font, Gradient, Length, Point, Subscription, Task, Theme,
    gradient::Linear,
    keyboard, mouse,
    widget::{
        Space, button, column, container, image, mouse_area, pick_list, radio, responsive, row,
        scrollable, slider, stack, svg, text, text_input, toggler, tooltip,
    },
};
use iconflow::{Pack, Size, Style, fonts, try_icon};
use iron_file_common::{
    browse_with_thumbnails,
    config::{BrowserLayout, BrowserSettings, ColorMode, ConfigStore, Profile, SidebarLocation},
    copy_entries, create_entry, create_symlinks, create_thumbnail, delete_entries, ensure_backend,
    pipe_backend_logs, proto, restart_backend, stream_directory,
};
use proto::{BrowseResponse, browse_response::Payload};
use serde::Deserialize;
use tokio::runtime::Runtime;

fn main() -> iced::Result {
    prefer_x11_when_available();
    if let Ok(runtime) = Runtime::new() {
        let _ = runtime.block_on(ensure_backend());
    }
    iced::application("Iron File", Gui::update, Gui::view)
        .theme(Gui::theme)
        .subscription(Gui::subscription)
        .run_with(|| {
            let gui = Gui::new();
            let task = gui.load_initial_directory();
            (gui, task)
        })
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
    drives: Vec<Drive>,
    mounts: Vec<SystemMount>,
    content: String,
    status: String,
    editing_address: bool,
    view: View,
    config_store: ConfigStore,
    profiles: Vec<Profile>,
    active_profile: Option<PathBuf>,
    new_profile_name: String,
    color_mode: ColorMode,
    light_accent_input: String,
    dark_accent_input: String,
    accent_picker: Option<AccentPickerState>,
    context_entry: Option<ContextEntry>,
    pointer_position: Point,
    context_position: Point,
    dragging_sidebar_location: Option<PathBuf>,
    sidebar_drop_target: Option<PathBuf>,
    sidebar_drop_at_end: bool,
    last_entry_click: Option<(PathBuf, Instant)>,
    terminal_recommendations: Vec<String>,
    history: Vec<PathBuf>,
    history_index: Option<usize>,
    sidebar_resize: Option<(f32, u16, u16)>,
    icon_themes: Vec<String>,
    entry_icons: HashMap<PathBuf, Option<PathBuf>>,
    thumbnail_handles: HashMap<PathBuf, image::Handle>,
    selected_entries: HashSet<PathBuf>,
    paste_buffer: Option<PasteBuffer>,
    pending_delete: Option<Vec<PathBuf>>,
    delete_confirm_selected: bool,
    pending_create: Option<(PathBuf, bool)>,
    create_entry_name: String,
    pending_profile_reset: bool,
    selection_anchor: Option<PathBuf>,
    modifiers: keyboard::Modifiers,
    browser_pointer: Point,
    rectangle_selection: Option<RectangleSelection>,
    tile_columns: Rc<Cell<usize>>,
}

const DEFAULT_TERMINAL_CHOICE: &str = "System default";
const CUSTOM_TERMINAL_CHOICE: &str = "Custom command";
const RECOMMENDED_TERMINALS: &[&str] = &[
    "gnome-terminal",
    "konsole",
    "xfce4-terminal",
    "mate-terminal",
    "lxterminal",
    "kitty",
    "alacritty",
    "wezterm",
    "foot",
    "urxvt",
    "xterm",
    "tilix",
];

#[derive(Debug, Clone)]
struct Drive {
    path: PathBuf,
    name: String,
    mount_points: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
struct SystemMount {
    path: PathBuf,
    filesystem: String,
}

#[derive(Debug, Clone)]
struct MountState {
    drives: Vec<Drive>,
    mounts: Vec<SystemMount>,
}

#[derive(Debug, Clone)]
struct ContextEntry {
    path: PathBuf,
    is_directory: bool,
    opener: Option<Result<String, String>>,
}

#[derive(Debug, Clone)]
struct RectangleSelection {
    start: Point,
    end: Point,
    initial_selection: HashSet<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Browser,
    Preferences,
}

#[derive(Debug, Clone, Copy)]
enum HistoryRequest {
    Initial,
    New,
    Existing(usize),
}

#[derive(Debug, Clone)]
enum BrowserCommand {
    CopySelection,
    Paste,
    DeleteSelection,
    AddSymlinkToPasteBuffer(PathBuf),
    CreateSymlinksHere(PathBuf),
}

#[derive(Debug, Clone, Copy)]
enum PasteMode {
    Copy,
    Symlink,
}

#[derive(Debug, Clone, Copy)]
struct AccentPickerState {
    dark: bool,
    hue: u16,
    saturation: u8,
    value: u8,
}

#[derive(Debug, Clone)]
struct PasteBuffer {
    entries: Vec<PathBuf>,
    mode: PasteMode,
}

#[derive(Debug, Clone)]
enum Message {
    AddressChanged(String),
    StartAddressEdit,
    CancelAddressEdit,
    OpenAddress,
    OpenPath(PathBuf),
    NavigateBack,
    NavigateForward,
    EntryClicked {
        path: PathBuf,
        is_directory: bool,
    },
    ExecuteBrowserCommand(BrowserCommand),
    FileCopyFinished(Result<Vec<PathBuf>, String>),
    ConfirmDelete,
    CancelDelete,
    SelectDeleteDialogAction(bool),
    ActivateDeleteDialogAction,
    FileDeleteFinished(Result<Vec<PathBuf>, String>),
    RequestCreateEntry {
        parent: PathBuf,
        is_directory: bool,
    },
    CreateEntryNameChanged(String),
    ConfirmCreateEntry,
    CancelCreateEntry,
    EntryCreated(Result<PathBuf, String>),
    ModifiersChanged(keyboard::Modifiers),
    StartRectangleSelection,
    RectanglePointerMoved(Point),
    FinishRectangleSelection,
    OpenParent,
    ShowBrowser,
    ShowPreferences,
    SelectProfile(PathBuf),
    NewProfileNameChanged(String),
    CreateProfile,
    RequestProfileReset,
    ConfirmProfileReset,
    CancelProfileReset,
    ColorModeSelected(ColorMode),
    OpenAccentPicker(bool),
    AccentHueChanged(u16),
    AccentSaturationChanged(u8),
    AccentValueChanged(u8),
    ConfirmAccentPicker,
    CancelAccentPicker,
    BrowserLayoutSelected(BrowserLayout),
    BrowserItemSizeChanged(u16),
    PreviewToggled(bool),
    SingleClickFoldersToggled(bool),
    TerminalChoiceSelected(String),
    TerminalCommandChanged(String),
    IconThemeSelected(String),
    ThumbnailLocationChanged(String),
    StartSidebarResize,
    FinishSidebarResize,
    ShowEntryContext {
        path: PathBuf,
        is_directory: bool,
    },
    FileOpenerResolved {
        path: PathBuf,
        opener: Result<String, String>,
    },
    ContextPointerMoved(Point),
    CloseFolderContext,
    OpenContextFile,
    OpenTerminalHere,
    AddContextFolderToSidebar,
    RemoveContextFolderFromSidebar,
    SidebarPressed(PathBuf),
    SidebarReleased(PathBuf),
    SidebarDragTarget(PathBuf),
    SidebarDragTargetCleared(PathBuf),
    SidebarDragTargetEnd,
    SidebarDragTargetEndCleared,
    SidebarReleasedAtEnd,
    MountsLoaded(Result<MountState, String>),
    MountDrive(PathBuf),
    FileOpened(Result<(), String>),
    TerminalOpened(Result<(), String>),
    BackendLogPipeEnded(Result<(), String>),
    RestartBackend,
    BackendRestarted(Result<(), String>),
    ThumbnailGenerated {
        path: PathBuf,
        thumbnail_path: Result<String, String>,
    },
    DirectoryEntryLoaded {
        directory: PathBuf,
        entry: Result<proto::FileEntry, String>,
    },
    BrowseFinished {
        result: Result<BrowseResponse, String>,
        history: HistoryRequest,
    },
    IconFontLoaded(Result<(), iced::font::Error>),
}

impl Gui {
    fn subscription(&self) -> Subscription<Message> {
        iced::event::listen_raw(|event, _, _| match event {
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                Some(Message::ModifiersChanged(modifiers))
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                if modifiers.command() =>
            {
                match key.as_ref() {
                    keyboard::Key::Character("c" | "C") => Some(Message::ExecuteBrowserCommand(
                        BrowserCommand::CopySelection,
                    )),
                    keyboard::Key::Character("v" | "V") => {
                        Some(Message::ExecuteBrowserCommand(BrowserCommand::Paste))
                    }
                    _ => None,
                }
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, .. })
                if key == keyboard::Key::Named(keyboard::key::Named::ArrowLeft) =>
            {
                Some(Message::SelectDeleteDialogAction(false))
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, .. })
                if key == keyboard::Key::Named(keyboard::key::Named::ArrowRight) =>
            {
                Some(Message::SelectDeleteDialogAction(true))
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
            _ => None,
        })
    }

    fn new() -> Self {
        let directory_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let config_store = ConfigStore::from_environment();
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
        Self {
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
            pointer_position: Point::ORIGIN,
            context_position: Point::ORIGIN,
            dragging_sidebar_location: None,
            sidebar_drop_target: None,
            sidebar_drop_at_end: false,
            last_entry_click: None,
            terminal_recommendations: recommended_terminal_commands(),
            history: Vec::new(),
            history_index: None,
            sidebar_resize: None,
            icon_themes: available_icon_themes(),
            entry_icons: HashMap::new(),
            thumbnail_handles: HashMap::new(),
            selected_entries: HashSet::new(),
            paste_buffer: None,
            pending_delete: None,
            delete_confirm_selected: false,
            pending_create: None,
            create_entry_name: String::new(),
            pending_profile_reset: false,
            selection_anchor: None,
            modifiers: keyboard::Modifiers::default(),
            browser_pointer: Point::ORIGIN,
            rectangle_selection: None,
            tile_columns: Rc::new(Cell::new(1)),
        }
    }

    fn load_initial_directory(&self) -> Task<Message> {
        let path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
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
                .chain(std::iter::once(Task::perform(
                    pipe_backend_logs(),
                    Message::BackendLogPipeEnded,
                ))),
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
                self.address = self.directory_path.display().to_string();
                self.editing_address = false;
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
                Task::none()
            }
            Message::NavigateBack => self.navigate_history(-1),
            Message::NavigateForward => self.navigate_history(1),
            Message::EntryClicked { path, is_directory } => {
                self.handle_entry_click(path, is_directory)
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
                Task::none()
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
            Message::OpenParent => {
                let parent = self.directory_path.parent().map(|path| path.to_path_buf());
                parent
                    .map(|path| self.open_path(path))
                    .unwrap_or_else(Task::none)
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
            Message::ColorModeSelected(color_mode) => {
                self.save_color_mode(color_mode);
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
            Message::BrowserItemSizeChanged(item_size) => {
                let mut browser = self.active_browser_settings();
                browser.item_size = item_size;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::PreviewToggled(preview_enabled) => {
                let mut browser = self.active_browser_settings();
                browser.preview_enabled = preview_enabled;
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
                self.context_entry = Some(ContextEntry {
                    path: path.clone(),
                    is_directory,
                    opener: None,
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
            Message::OpenContextFile => {
                let Some(context_entry) = self.context_entry.take() else {
                    return Task::none();
                };
                Task::perform(open_file(context_entry.path), Message::FileOpened)
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
                self.dragging_sidebar_location = Some(path);
                self.sidebar_drop_target = None;
                self.sidebar_drop_at_end = false;
                Task::none()
            }
            Message::SidebarReleased(path) => self.release_sidebar_location(path),
            Message::SidebarDragTarget(path) => {
                if self.dragging_sidebar_location.is_some() {
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
                eprintln!("[iron-file thumbnails] {error}");
                Task::none()
            }
            Message::DirectoryEntryLoaded {
                directory,
                entry: Ok(entry),
            } => {
                if self.directory_path != directory {
                    return Task::none();
                }
                let path = PathBuf::from(&entry.path);
                let icon_theme = self.active_browser_settings().icon_theme;
                self.entry_icons
                    .insert(path.clone(), themed_entry_icon_path(&icon_theme, &entry));
                let is_directory = entry.is_directory;
                self.entries.push(entry);
                sort_entries(&mut self.entries);
                self.status = format!("{} entries", self.entries.len());
                if is_directory {
                    Task::none()
                } else {
                    let thumbnail_directory = self.active_browser_settings().thumbnail_location;
                    Task::perform(
                        create_thumbnail(path.clone(), thumbnail_directory),
                        move |thumbnail_path| Message::ThumbnailGenerated {
                            path: path.clone(),
                            thumbnail_path,
                        },
                    )
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
            ColorMode::System => Theme::default(),
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
        Theme::custom("Iron File".into(), palette)
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
                container(Space::new(Length::Fixed(20.0), Length::Fixed(20.0)))
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

    fn active_browser_settings(&self) -> BrowserSettings {
        self.active_profile
            .as_deref()
            .and_then(|path| self.profiles.iter().find(|profile| profile.path == path))
            .map(|profile| profile.browser.clone())
            .unwrap_or_else(iron_file_common::config::default_browser_settings)
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
        self.entry_icons = self
            .entries
            .iter()
            .map(|entry| {
                let path = PathBuf::from(&entry.path);
                let icon = themed_entry_icon_path(&icon_theme, entry);
                (path, icon)
            })
            .collect();
    }

    fn handle_entry_click(&mut self, path: PathBuf, is_directory: bool) -> Task<Message> {
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
        }
    }

    fn update_rectangle_selection(&mut self) {
        let Some(selection) = self.rectangle_selection.clone() else {
            return;
        };

        let left = selection.start.x.min(selection.end.x);
        let right = selection.start.x.max(selection.end.x);
        let top = selection.start.y.min(selection.end.y);
        let bottom = selection.start.y.max(selection.end.y);
        let browser = self.active_browser_settings();
        let tile_width = f32::from(browser.item_size) * 3.5;
        let tile_height = tile_width * 1.2;
        let row_height = f32::from(browser.item_size).max(24.0) + 12.0;
        let columns = self.tile_columns.get().max(1);

        self.selected_entries = selection.initial_selection;
        for (index, entry) in self.entries.iter().enumerate() {
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

    fn save_browser_settings(&mut self, browser: BrowserSettings) {
        let Some(path) = self.active_profile.clone() else {
            self.status = "No active configuration profile".into();
            return;
        };
        let Some(profile) = self.profiles.iter().find(|profile| profile.path == path) else {
            self.status = "The active configuration profile is unavailable".into();
            return;
        };
        match self.config_store.save_browser_settings(profile, browser) {
            Ok(saved_profile) => self.apply_saved_profile(saved_profile),
            Err(error) => self.status = error,
        }
    }

    fn select_profile(&mut self, path: PathBuf) {
        let Some(profile) = self.profiles.iter().find(|profile| profile.path == path) else {
            return;
        };
        self.active_profile = Some(path.clone());
        self.color_mode = profile.color_mode;
        self.light_accent_input = profile.theme.light_highlight.clone();
        self.dark_accent_input = profile.theme.dark_highlight.clone();
        self.refresh_entry_icons();
        if let Err(error) = self.config_store.set_active_profile(&path) {
            self.status = error;
        }
    }

    fn create_profile(&mut self) {
        match self.config_store.create_profile(&self.new_profile_name) {
            Ok(profile) => {
                let path = profile.path.clone();
                self.profiles.push(profile);
                self.profiles
                    .sort_by(|left, right| left.name.cmp(&right.name));
                self.new_profile_name.clear();
                self.select_profile(path);
            }
            Err(error) => self.status = error,
        }
    }

    fn save_color_mode(&mut self, color_mode: ColorMode) {
        let Some(path) = self.active_profile.clone() else {
            self.status = "No active configuration profile".into();
            return;
        };
        let Some(profile) = self.profiles.iter().find(|profile| profile.path == path) else {
            self.status = "The active configuration profile is unavailable".into();
            return;
        };
        match self.config_store.save_color_mode(profile, color_mode) {
            Ok(saved_profile) => self.apply_saved_profile(saved_profile),
            Err(error) => self.status = error,
        }
    }

    fn save_accent_color(&mut self, dark: bool, value: String) {
        if dark {
            self.dark_accent_input = value.clone();
        } else {
            self.light_accent_input = value.clone();
        }
        if parse_color(&value).is_none() {
            self.status = "Accent color must be a hex color, for example #4f7cac".into();
            return;
        }
        let Some(path) = self.active_profile.clone() else {
            self.status = "No active configuration profile".into();
            return;
        };
        let Some(profile) = self.profiles.iter().find(|profile| profile.path == path) else {
            self.status = "The active configuration profile is unavailable".into();
            return;
        };
        let mut theme = profile.theme.clone();
        if dark {
            theme.dark_highlight = value;
        } else {
            theme.light_highlight = value;
        }
        match self.config_store.save_theme_settings(profile, theme) {
            Ok(saved_profile) => self.apply_saved_profile(saved_profile),
            Err(error) => self.status = error,
        }
    }

    fn save_sidebar_locations(&mut self, sidebar_locations: Vec<SidebarLocation>) {
        let Some(path) = self.active_profile.clone() else {
            self.status = "No active configuration profile".into();
            return;
        };
        let Some(profile) = self.profiles.iter().find(|profile| profile.path == path) else {
            self.status = "The active configuration profile is unavailable".into();
            return;
        };
        match self
            .config_store
            .save_sidebar_locations(profile, sidebar_locations)
        {
            Ok(saved_profile) => self.apply_saved_profile(saved_profile),
            Err(error) => self.status = error,
        }
    }

    fn apply_saved_profile(&mut self, saved_profile: Profile) {
        let saved_path = saved_profile.path.clone();
        let color_mode = saved_profile.color_mode;
        self.light_accent_input = saved_profile.theme.light_highlight.clone();
        self.dark_accent_input = saved_profile.theme.dark_highlight.clone();
        if let Some(index) = self
            .profiles
            .iter()
            .position(|profile| profile.path == saved_path)
        {
            self.profiles[index] = saved_profile;
        } else {
            self.profiles.push(saved_profile);
            self.profiles
                .sort_by(|left, right| left.name.cmp(&right.name));
        }
        self.active_profile = Some(saved_path.clone());
        self.color_mode = color_mode;
        self.refresh_entry_icons();
        if let Err(error) = self.config_store.set_active_profile(&saved_path) {
            self.status = error;
        }
    }

    fn reset_active_profile(&mut self) {
        let Some(path) = self.active_profile.clone() else {
            self.status = "No active configuration profile".into();
            return;
        };
        let Some(profile) = self.profiles.iter().find(|profile| profile.path == path) else {
            self.status = "The active configuration profile is unavailable".into();
            return;
        };
        match self.config_store.reset_profile(profile) {
            Ok(saved_profile) => self.apply_saved_profile(saved_profile),
            Err(error) => self.status = error,
        }
    }

    fn active_sidebar_locations(&self) -> Vec<SidebarLocation> {
        self.active_profile
            .as_deref()
            .and_then(|path| self.profiles.iter().find(|profile| profile.path == path))
            .map(|profile| profile.sidebar_locations.clone())
            .unwrap_or_default()
    }

    fn add_context_folder_to_sidebar(&mut self) {
        let Some(ContextEntry {
            path,
            is_directory: true,
            ..
        }) = self.context_entry.take()
        else {
            return;
        };
        let mut locations = self.active_sidebar_locations();
        if locations.iter().any(|location| location.path == path) {
            return;
        }
        let label = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| path.display().to_string());
        locations.push(SidebarLocation { label, path });
        self.save_sidebar_locations(locations);
    }

    fn remove_context_folder_from_sidebar(&mut self) {
        let Some(ContextEntry {
            path,
            is_directory: true,
            ..
        }) = self.context_entry.take()
        else {
            return;
        };
        let mut locations = self.active_sidebar_locations();
        locations.retain(|location| location.path != path);
        self.save_sidebar_locations(locations);
    }

    fn release_sidebar_location(&mut self, target: PathBuf) -> Task<Message> {
        let Some(source) = self.dragging_sidebar_location.take() else {
            return Task::none();
        };
        self.sidebar_drop_target = None;
        self.sidebar_drop_at_end = false;
        if source == target {
            return self.open_path(target);
        }
        let mut locations = self.active_sidebar_locations();
        let Some(source_index) = locations
            .iter()
            .position(|location| location.path == source)
        else {
            return Task::none();
        };
        let location = locations.remove(source_index);
        let Some(target_index) = locations
            .iter()
            .position(|location| location.path == target)
        else {
            return Task::none();
        };
        locations.insert(target_index, location);
        self.save_sidebar_locations(locations);
        Task::none()
    }

    fn release_sidebar_location_at_end(&mut self) -> Task<Message> {
        let Some(source) = self.dragging_sidebar_location.take() else {
            return Task::none();
        };
        self.sidebar_drop_target = None;
        self.sidebar_drop_at_end = false;
        let mut locations = self.active_sidebar_locations();
        let Some(source_index) = locations
            .iter()
            .position(|location| location.path == source)
        else {
            return Task::none();
        };
        let location = locations.remove(source_index);
        locations.push(location);
        self.save_sidebar_locations(locations);
        Task::none()
    }

    fn open_path(&mut self, path: PathBuf) -> Task<Message> {
        self.request_path(path, HistoryRequest::New)
    }

    fn navigate_history(&mut self, direction: isize) -> Task<Message> {
        let Some(index) = self.history_index else {
            return Task::none();
        };
        let Some(target_index) = index.checked_add_signed(direction) else {
            return Task::none();
        };
        let Some(path) = self.history.get(target_index).cloned() else {
            return Task::none();
        };
        self.request_path(path, HistoryRequest::Existing(target_index))
    }

    fn request_path(&mut self, path: PathBuf, history: HistoryRequest) -> Task<Message> {
        self.editing_address = false;
        self.status = format!("Loading {}", path.display());
        let thumbnail_directory = self.active_browser_settings().thumbnail_location;
        Task::perform(
            browse_with_thumbnails(path, Some(thumbnail_directory)),
            move |result| Message::BrowseFinished { result, history },
        )
    }

    fn apply_response(
        &mut self,
        result: Result<BrowseResponse, String>,
        history: HistoryRequest,
    ) -> Task<Message> {
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                self.status = error;
                return Task::none();
            }
        };

        match response.payload {
            Some(Payload::Directory(directory)) => {
                self.address = response.path.clone();
                self.directory_path = PathBuf::from(response.path);
                self.record_history(self.directory_path.clone(), history);
                let _ = directory;
                self.entries.clear();
                self.selected_entries.clear();
                self.selection_anchor = None;
                self.thumbnail_handles.clear();
                self.refresh_entry_icons();
                self.content.clear();
                self.status = "Loading folder contents".into();
                let directory = self.directory_path.clone();
                Task::run(stream_directory(directory.clone()), move |entry| {
                    Message::DirectoryEntryLoaded {
                        directory: directory.clone(),
                        entry,
                    }
                })
            }
            Some(Payload::File(file)) => {
                self.address = response.path;
                self.content = file.content;
                self.status = "File preview".into();
                Task::none()
            }
            Some(Payload::Error(error)) => {
                self.status = error.message;
                Task::none()
            }
            None => {
                self.status = "Backend returned an invalid response".into();
                Task::none()
            }
        }
    }

    fn record_history(&mut self, path: PathBuf, request: HistoryRequest) {
        match request {
            HistoryRequest::Initial => {
                self.history = vec![path];
                self.history_index = Some(0);
            }
            HistoryRequest::New => {
                let Some(index) = self.history_index else {
                    self.history = vec![path];
                    self.history_index = Some(0);
                    return;
                };
                if self.history.get(index) == Some(&path) {
                    return;
                }
                self.history.truncate(index + 1);
                self.history.push(path);
                self.history_index = Some(self.history.len() - 1);
            }
            HistoryRequest::Existing(index) if self.history.get(index) == Some(&path) => {
                self.history_index = Some(index);
            }
            HistoryRequest::Existing(_) => self.record_history(path, HistoryRequest::New),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        match self.view {
            View::Browser => self.browser_view(),
            View::Preferences => self.preferences_view(),
        }
    }

    fn browser_view(&self) -> Element<'_, Message> {
        let browser_settings = self.active_browser_settings();
        let entries = self.entries.iter().fold(column![], |column, entry| {
            let icon = self.entry_icon(entry, browser_settings.item_size);
            let path = PathBuf::from(&entry.path);
            let is_selected = self.selected_entries.contains(&path);
            if entry.is_directory {
                column.push(
                    mouse_area(
                        button(row![icon, text(&entry.name)].spacing(8))
                            .style(move |theme, status| {
                                file_item_button_style(theme, status, is_selected)
                            })
                            .width(Length::Fill)
                            .on_press(Message::EntryClicked {
                                path: path.clone(),
                                is_directory: true,
                            }),
                    )
                    .on_right_press(Message::ShowEntryContext {
                        path: path.clone(),
                        is_directory: true,
                    })
                    .interaction(mouse::Interaction::Pointer),
                )
            } else {
                column.push(
                    mouse_area(
                        button(row![icon, text(&entry.name)].spacing(8))
                            .style(move |theme, status| {
                                file_item_button_style(theme, status, is_selected)
                            })
                            .width(Length::Fill)
                            .on_press(Message::EntryClicked {
                                path: path.clone(),
                                is_directory: false,
                            }),
                    )
                    .on_right_press(Message::ShowEntryContext {
                        path,
                        is_directory: false,
                    }),
                )
            }
        });
        let entries: Element<'_, Message> = if browser_settings.layout == BrowserLayout::Tiles {
            let tile_columns = Rc::clone(&self.tile_columns);
            responsive(move |size| {
                let tile_width = f32::from(browser_settings.item_size) * 3.5;
                let tile_height = tile_width * 1.2;
                let columns = (size.width / tile_width).floor().max(1.0) as usize;
                tile_columns.set(columns);
                let tiles =
                    self.entries
                        .chunks(columns)
                        .fold(column![].spacing(8), |column, chunk| {
                            let tiles = chunk.iter().fold(row![].spacing(8), |row, entry| {
                                let path = PathBuf::from(&entry.path);
                                let is_selected = self.selected_entries.contains(&path);
                                let icon = self.entry_icon(
                                    entry,
                                    browser_settings.item_size.saturating_mul(9) / 5,
                                );
                                let tile_content = container(
                                    column![
                                        icon,
                                        text(&entry.name)
                                            .width(Length::Fill)
                                            .align_x(iced::alignment::Horizontal::Center),
                                    ]
                                    .spacing(6)
                                    .align_x(iced::alignment::Horizontal::Center),
                                )
                                .width(Length::Fill)
                                .height(Length::Fill)
                                .center_x(Length::Fill)
                                .center_y(Length::Fill);
                                let tile = button(tile_content)
                                    .style(move |theme, status| {
                                        file_item_button_style(theme, status, is_selected)
                                    })
                                    .width(Length::Fixed(tile_width))
                                    .height(Length::Fixed(tile_height))
                                    .on_press(Message::EntryClicked {
                                        path: path.clone(),
                                        is_directory: entry.is_directory,
                                    });
                                row.push(
                                    mouse_area(tile)
                                        .on_right_press(Message::ShowEntryContext {
                                            path,
                                            is_directory: entry.is_directory,
                                        })
                                        .interaction(mouse::Interaction::Pointer),
                                )
                            });
                            column.push(tiles)
                        });

                scrollable(tiles)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            })
            .into()
        } else {
            entries.into()
        };

        let can_go_back = self.history_index.is_some_and(|index| index > 0);
        let can_go_forward = self
            .history_index
            .is_some_and(|index| index + 1 < self.history.len());
        let mut address_bar = row![
            tooltip(
                button(icon_text("arrow-left"))
                    .on_press_maybe(can_go_back.then_some(Message::NavigateBack)),
                text("Back"),
                tooltip::Position::Bottom,
            ),
            tooltip(
                button(icon_text("arrow-right"))
                    .on_press_maybe(can_go_forward.then_some(Message::NavigateForward)),
                text("Forward"),
                tooltip::Position::Bottom,
            ),
            tooltip(
                button(icon_text("folder-up")).on_press(Message::OpenParent),
                text("Parent folder"),
                tooltip::Position::Bottom,
            ),
            self.address_control(),
        ]
        .spacing(8);
        if self.editing_address {
            address_bar = address_bar.push(tooltip(
                button(icon_text("folder-open")).on_press(Message::OpenAddress),
                text(String::from("Open path")),
                tooltip::Position::Bottom,
            ));
        }
        address_bar = address_bar.push(tooltip(
            button(icon_text("settings")).on_press(Message::ShowPreferences),
            text(String::from("Preferences")),
            tooltip::Position::Bottom,
        ));
        let tiles_layout = browser_settings.layout == BrowserLayout::Tiles;
        let browser: Element<'_, Message> = if browser_settings.preview_enabled {
            let file_pane: Element<'_, Message> = if tiles_layout {
                container(entries)
                    .width(Length::FillPortion(1))
                    .height(Length::Fill)
                    .into()
            } else {
                scrollable(entries).width(Length::FillPortion(1)).into()
            };
            row![
                file_pane,
                scrollable(text(&self.content)).width(Length::FillPortion(2)),
            ]
            .spacing(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else if tiles_layout {
            entries
        } else {
            scrollable(entries)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };
        let selection_overlay: Option<Element<'_, Message>> =
            self.rectangle_selection.as_ref().map(|selection| {
                let left = selection.start.x.min(selection.end.x);
                let top = selection.start.y.min(selection.end.y);
                let width = (selection.start.x - selection.end.x).abs().max(1.0);
                let height = (selection.start.y - selection.end.y).abs().max(1.0);
                container(column![
                    Space::with_height(top),
                    row![
                        Space::with_width(left),
                        container(Space::new(Length::Fixed(width), Length::Fixed(height))).style(
                            |theme: &Theme| {
                                iced::widget::container::Style::default()
                                    .background(Color::from_rgba8(90, 130, 200, 0.22))
                                    .border(Border {
                                        color: theme.palette().primary,
                                        width: 1.0,
                                        radius: 4.0.into(),
                                    })
                            }
                        ),
                    ],
                ])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            });
        let browser: Element<'_, Message> = stack![browser]
            .push_maybe(selection_overlay)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        let browser = mouse_area(browser)
            .on_right_press(Message::ShowEntryContext {
                path: self.directory_path.clone(),
                is_directory: true,
            })
            .on_press(Message::StartRectangleSelection)
            .on_move(Message::RectanglePointerMoved)
            .on_release(Message::FinishRectangleSelection);
        let resize_handle = mouse_area(
            container(Space::new(Length::Fixed(6.0), Length::Fill))
                .width(Length::Fixed(6.0))
                .height(Length::Fill)
                .style(|_| {
                    iced::widget::container::Style::default()
                        .background(Color::from_rgba8(128, 128, 128, 0.25))
                }),
        )
        .on_press(Message::StartSidebarResize)
        .on_release(Message::FinishSidebarResize)
        .interaction(mouse::Interaction::ResizingHorizontally);
        let sidebar_panel = row![self.sidebar_view(), resize_handle]
            .spacing(0)
            .height(Length::Fill);
        let main_content = row![sidebar_panel, browser]
            .spacing(16)
            .width(Length::Fill)
            .height(Length::Fill);
        let content = column![address_bar, text(&self.status), main_content];

        let page = container(content.spacing(12).padding(16).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill);
        let overlay = self.context_entry.as_ref().map(|entry| {
            let action: Element<'_, Message> = if entry.is_directory {
                let is_in_sidebar = self
                    .active_sidebar_locations()
                    .iter()
                    .any(|location| location.path == entry.path);
                if is_in_sidebar {
                    button(
                        row![
                            icon_text("folder-minus").size(16),
                            text("Remove from sidebar")
                        ]
                        .spacing(8),
                    )
                    .width(Length::Fill)
                    .style(context_menu_button_style)
                    .on_press(Message::RemoveContextFolderFromSidebar)
                    .into()
                } else {
                    button(
                        row![icon_text("folder-plus").size(16), text("Add to sidebar")].spacing(8),
                    )
                    .width(Length::Fill)
                    .style(context_menu_button_style)
                    .on_press(Message::AddContextFolderToSidebar)
                    .into()
                }
            } else {
                match &entry.opener {
                    Some(Ok(application)) => button(
                        row![
                            icon_text("external-link").size(16),
                            text(format!("Open (with {application})"))
                        ]
                        .spacing(8),
                    )
                    .width(Length::Fill)
                    .style(context_menu_button_style)
                    .on_press(Message::OpenContextFile)
                    .into(),
                    Some(Err(_)) => {
                        button(row![icon_text("external-link").size(16), text("Open")].spacing(8))
                            .width(Length::Fill)
                            .style(context_menu_button_style)
                            .on_press(Message::OpenContextFile)
                            .into()
                    }
                    None => text("Finding default application...").into(),
                }
            };
            let mut actions = column![action].spacing(4);
            if !self.selected_entries.is_empty() {
                actions = actions.push(
                    button(row![icon_text("copy").size(16), text("Copy selection")].spacing(8))
                        .width(Length::Fill)
                        .style(context_menu_button_style)
                        .on_press(Message::ExecuteBrowserCommand(
                            BrowserCommand::CopySelection,
                        )),
                );
                actions = actions.push(
                    button(
                        row![icon_text("trash-2").size(16), text("Delete selection")].spacing(8),
                    )
                    .width(Length::Fill)
                    .style(context_menu_button_style)
                    .on_press(Message::ExecuteBrowserCommand(
                        BrowserCommand::DeleteSelection,
                    )),
                );
            }
            if self.paste_buffer.is_some() {
                actions = actions.push(
                    button(row![icon_text("clipboard-paste").size(16), text("Paste")].spacing(8))
                        .width(Length::Fill)
                        .style(context_menu_button_style)
                        .on_press(Message::ExecuteBrowserCommand(BrowserCommand::Paste)),
                );
            }
            if entry.is_directory {
                actions = actions.push(
                    button(
                        row![icon_text("folder-plus").size(16), text("Create folder")].spacing(8),
                    )
                    .width(Length::Fill)
                    .style(context_menu_button_style)
                    .on_press(Message::RequestCreateEntry {
                        parent: entry.path.clone(),
                        is_directory: true,
                    }),
                );
                actions = actions.push(
                    button(row![icon_text("file-plus").size(16), text("Create file")].spacing(8))
                        .width(Length::Fill)
                        .style(context_menu_button_style)
                        .on_press(Message::RequestCreateEntry {
                            parent: entry.path.clone(),
                            is_directory: false,
                        }),
                );
                actions = actions.push(
                    button(
                        row![icon_text("link").size(16), text("Create symlink here")].spacing(8),
                    )
                    .width(Length::Fill)
                    .style(context_menu_button_style)
                    .on_press(Message::ExecuteBrowserCommand(
                        BrowserCommand::CreateSymlinksHere(entry.path.clone()),
                    )),
                );
                actions = actions.push(
                    button(
                        row![
                            icon_text("link").size(16),
                            text("Add symlink to paste buffer")
                        ]
                        .spacing(8),
                    )
                    .width(Length::Fill)
                    .style(context_menu_button_style)
                    .on_press(Message::ExecuteBrowserCommand(
                        BrowserCommand::AddSymlinkToPasteBuffer(entry.path.clone()),
                    )),
                );
                actions = actions.push(
                    button(
                        row![icon_text("terminal").size(16), text("Open terminal here")].spacing(8),
                    )
                    .width(Length::Fill)
                    .style(context_menu_button_style)
                    .on_press(Message::OpenTerminalHere),
                );
            }
            let menu = container(actions)
                .width(Length::Fixed(240.0))
                .padding(8)
                .style(|theme: &Theme| {
                    iced::widget::container::Style::default()
                        .background(theme.palette().background)
                        .border(Border {
                            color: Color::from_rgba8(128, 128, 128, 0.45),
                            width: 1.0,
                            radius: 6.0.into(),
                        })
                });
            let menu_position = container(column![
                Space::with_height(self.context_position.y),
                row![Space::with_width(self.context_position.x), menu],
            ])
            .width(Length::Fill)
            .height(Length::Fill);
            stack![
                mouse_area(Space::new(Length::Fill, Length::Fill))
                    .on_press(Message::CloseFolderContext)
                    .on_right_press(Message::CloseFolderContext),
                menu_position,
            ]
            .width(Length::Fill)
            .height(Length::Fill)
        });

        let page: Element<'_, Message> = if self.editing_address {
            stack![
                mouse_area(Space::new(Length::Fill, Length::Fill))
                    .on_press(Message::CancelAddressEdit)
                    .on_move(Message::ContextPointerMoved)
                    .on_release(Message::FinishSidebarResize),
                page,
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            mouse_area(page)
                .on_move(Message::ContextPointerMoved)
                .on_release(Message::FinishSidebarResize)
                .into()
        };
        let delete_confirmation = self.pending_delete.as_ref().map(|paths| {
            let delete_confirm_selected = self.delete_confirm_selected;
            let dialog = container(
                column![
                    text(format!("Delete {} item(s)?", paths.len())),
                    text("This permanently deletes the selected files and folders."),
                    row![
                        button(text("Cancel"))
                            .style(move |theme, status| {
                                if !delete_confirm_selected {
                                    button::primary(theme, status)
                                } else {
                                    button::secondary(theme, status)
                                }
                            })
                            .on_press(Message::CancelDelete),
                        button(text("Delete"))
                            .style(move |theme, status| {
                                if delete_confirm_selected {
                                    button::primary(theme, status)
                                } else {
                                    button::secondary(theme, status)
                                }
                            })
                            .on_press(Message::ConfirmDelete),
                    ]
                    .spacing(8),
                ]
                .spacing(12),
            )
            .padding(16)
            .style(|theme: &Theme| {
                iced::widget::container::Style::default()
                    .background(theme.palette().background)
                    .border(Border {
                        color: theme.palette().primary,
                        width: 1.0,
                        radius: 6.0.into(),
                    })
            });
            stack![
                mouse_area(Space::new(Length::Fill, Length::Fill)).on_press(Message::CancelDelete),
                container(dialog)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
        });
        let create_dialog = self.pending_create.as_ref().map(|(_, is_directory)| {
            let kind = if *is_directory { "folder" } else { "file" };
            let dialog = container(
                column![
                    text(format!("Create {kind}")),
                    text_input("Name", &self.create_entry_name)
                        .on_input(Message::CreateEntryNameChanged)
                        .on_submit(Message::ConfirmCreateEntry),
                    row![
                        button(text("Cancel")).on_press(Message::CancelCreateEntry),
                        button(text("Create")).on_press(Message::ConfirmCreateEntry),
                    ]
                    .spacing(8),
                ]
                .spacing(12),
            )
            .padding(16)
            .style(|theme: &Theme| {
                iced::widget::container::Style::default()
                    .background(theme.palette().background)
                    .border(Border {
                        color: theme.palette().primary,
                        width: 1.0,
                        radius: 6.0.into(),
                    })
            });
            stack![
                mouse_area(Space::new(Length::Fill, Length::Fill))
                    .on_press(Message::CancelCreateEntry),
                container(dialog)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
        });

        stack![page]
            .push_maybe(overlay)
            .push_maybe(delete_confirmation)
            .push_maybe(create_dialog)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn entry_icon<'a>(&self, entry: &proto::FileEntry, size: u16) -> Element<'a, Message> {
        let opacity = entry.name.starts_with('.').then_some(0.55).unwrap_or(1.0);
        let icon: Element<'a, Message> =
            if let Some(handle) = self.thumbnail_handles.get(&PathBuf::from(&entry.path)) {
                image(handle.clone())
                    .width(Length::Fixed(f32::from(size)))
                    .height(Length::Fixed(f32::from(size)))
                    .opacity(opacity)
                    .into()
            } else if let Some(path) = self
                .entry_icons
                .get(&PathBuf::from(&entry.path))
                .and_then(|path| path.as_ref())
            {
                svg(svg::Handle::from_path(path))
                    .width(Length::Fixed(f32::from(size)))
                    .height(Length::Fixed(f32::from(size)))
                    .opacity(opacity)
                    .into()
            } else {
                let icon = icon_text(if entry.is_directory { "folder" } else { "file" }).size(size);
                if entry.name.starts_with('.') {
                    icon.color(Color::from_rgba8(128, 128, 128, opacity)).into()
                } else {
                    icon.into()
                }
            };
        if !entry.is_symlink {
            return icon;
        }

        let badge_edge = (f32::from(size) * 0.24).clamp(10.0, 18.0);
        let badge = container(
            icon_text("link")
                .size((badge_edge - 4.0).max(7.0) as u16)
                .color(Color::from_rgb8(35, 35, 35)),
        )
        .width(Length::Fixed(badge_edge))
        .height(Length::Fixed(badge_edge))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(|_| {
            iced::widget::container::Style::default()
                .background(Color::from_rgba8(235, 235, 235, 0.92))
                .border(Border {
                    color: Color::from_rgba8(70, 70, 70, 0.7),
                    width: 1.0,
                    radius: 3.0.into(),
                })
        });
        stack![
            container(icon)
                .width(Length::Fixed(f32::from(size)))
                .height(Length::Fixed(f32::from(size))),
            container(badge)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::alignment::Horizontal::Right)
                .align_y(iced::alignment::Vertical::Bottom),
        ]
        .width(Length::Fixed(f32::from(size)))
        .height(Length::Fixed(f32::from(size)))
        .into()
    }

    fn address_control(&self) -> Element<'_, Message> {
        if self.editing_address {
            return text_input("Path", &self.address)
                .on_input(Message::AddressChanged)
                .on_submit(Message::OpenAddress)
                .width(Length::Fill)
                .into();
        }

        let path = PathBuf::from(&self.address);
        let mut target = PathBuf::new();
        let mut breadcrumbs = row![].spacing(2);
        for component in path.components() {
            use std::path::Component;

            let label = match component {
                Component::RootDir => {
                    target.push(component.as_os_str());
                    "/".into()
                }
                Component::CurDir => {
                    target.push(component.as_os_str());
                    ".".into()
                }
                Component::ParentDir => {
                    target.push(component.as_os_str());
                    "..".into()
                }
                Component::Normal(name) => {
                    target.push(name);
                    name.to_string_lossy().into_owned()
                }
                Component::Prefix(prefix) => {
                    target.push(prefix.as_os_str());
                    prefix.as_os_str().to_string_lossy().into_owned()
                }
            };
            breadcrumbs = breadcrumbs.push(
                button(text(label))
                    .style(iced::widget::button::text)
                    .on_press(Message::OpenPath(target.clone())),
            );
        }

        let breadcrumbs = container(breadcrumbs)
            .padding([2, 6])
            .width(Length::Fill)
            .align_y(iced::alignment::Vertical::Center)
            .style(|theme| {
                iced::widget::container::Style::default().border(Border {
                    color: theme.extended_palette().background.strong.color,
                    width: 1.0,
                    radius: 4.0.into(),
                })
            });
        stack![
            mouse_area(Space::new(Length::Fill, Length::Fixed(30.0)))
                .on_press(Message::StartAddressEdit),
            breadcrumbs,
        ]
        .width(Length::Fill)
        .height(Length::Shrink)
        .into()
    }

    fn sidebar_view(&self) -> Element<'_, Message> {
        let locations = self.active_sidebar_locations().into_iter().fold(
            column![text("Locations").size(16)].spacing(6),
            |column, location| {
                let is_dragging = self.dragging_sidebar_location.as_ref() == Some(&location.path);
                let is_open = self.directory_path == location.path;
                let is_drop_target = self.dragging_sidebar_location.is_some()
                    && !is_dragging
                    && self.sidebar_drop_target.as_ref() == Some(&location.path);
                let label = if is_dragging {
                    format!("Moving {}", location.label)
                } else {
                    location.label.clone()
                };
                let item =
                    container(row![icon_text(sidebar_icon(&location)), text(label)].spacing(8))
                        .padding(8)
                        .width(Length::Fill);
                let item = if is_open {
                    item.style(|theme| {
                        iced::widget::container::Style::default()
                            .background(theme.palette().primary)
                            .color(theme.palette().background)
                    })
                } else {
                    item
                };
                let item: Element<'_, Message> = if is_drop_target {
                    stack![
                        item,
                        container(Space::with_height(Length::Fixed(2.0)))
                            .width(Length::Fill)
                            .height(Length::Fixed(2.0))
                            .style(|theme: &Theme| {
                                iced::widget::container::Style::default()
                                    .background(theme.palette().primary)
                            }),
                    ]
                    .width(Length::Fill)
                    .into()
                } else {
                    item.into()
                };
                column.push(
                    mouse_area(item)
                        .on_press(Message::SidebarPressed(location.path.clone()))
                        .on_release(Message::SidebarReleased(location.path.clone()))
                        .on_right_press(Message::ShowEntryContext {
                            path: location.path.clone(),
                            is_directory: true,
                        })
                        .on_enter(Message::SidebarDragTarget(location.path.clone()))
                        .on_exit(Message::SidebarDragTargetCleared(location.path)),
                )
            },
        );
        let mounted = self.mounts.iter().fold(
            column![text("Mounted").size(14)].spacing(4),
            |column, mount| {
                column.push(
                    button(
                        row![
                            icon_text("hard-drive"),
                            text(mount.path.display().to_string()),
                        ]
                        .spacing(8),
                    )
                    .style(iced::widget::button::text)
                    .width(Length::Fill)
                    .on_press(Message::OpenPath(mount.path.clone())),
                )
            },
        );
        let unmounted = self
            .drives
            .iter()
            .filter(|drive| drive.mount_points.is_empty())
            .fold(
                column![text("Available").size(14)].spacing(4),
                |column, drive| {
                    column.push(
                        row![
                            container(row![icon_text("hard-drive"), text(&drive.name)].spacing(8))
                                .width(Length::Fill),
                            tooltip(
                                button(icon_text("plug-zap"))
                                    .on_press(Message::MountDrive(drive.path.clone())),
                                text("Mount drive"),
                                tooltip::Position::Right,
                            ),
                        ]
                        .spacing(4),
                    )
                },
            );
        let mounts = column![text("Mounts").size(16), mounted, unmounted].spacing(6);
        let drop_zone_height = if self.dragging_sidebar_location.is_some() {
            20.0
        } else {
            4.0
        };
        let end_drop_zone = mouse_area(
            container(Space::with_height(Length::Fixed(drop_zone_height)))
                .width(Length::Fill)
                .height(Length::Fixed(drop_zone_height))
                .style(move |theme: &Theme| {
                    if self.sidebar_drop_at_end {
                        iced::widget::container::Style::default()
                            .background(theme.palette().primary)
                    } else {
                        iced::widget::container::Style::default()
                    }
                }),
        )
        .on_release(Message::SidebarReleasedAtEnd)
        .on_enter(Message::SidebarDragTargetEnd)
        .on_exit(Message::SidebarDragTargetEndCleared);
        let sidebar_content = column![locations, end_drop_zone, mounts].spacing(20);
        container(scrollable(sidebar_content))
            .width(Length::Fixed(f32::from(self.sidebar_width())))
            .height(Length::Fill)
            .into()
    }

    fn preferences_view(&self) -> Element<'_, Message> {
        let back_button = tooltip(
            button(icon_text("arrow-left")).on_press(Message::ShowBrowser),
            text("Back to files"),
            tooltip::Position::Bottom,
        );
        let options = column![
            radio(
                "Day",
                ColorMode::Day,
                Some(self.color_mode),
                Message::ColorModeSelected,
            ),
            radio(
                "Night",
                ColorMode::Night,
                Some(self.color_mode),
                Message::ColorModeSelected,
            ),
            radio(
                "System",
                ColorMode::System,
                Some(self.color_mode),
                Message::ColorModeSelected,
            ),
        ]
        .spacing(12);
        let browser = self.active_browser_settings();
        let thumbnail_location = browser.thumbnail_location.display().to_string();
        let browser_options = column![
            radio(
                "List",
                BrowserLayout::List,
                Some(browser.layout),
                Message::BrowserLayoutSelected
            ),
            radio(
                "Tiles",
                BrowserLayout::Tiles,
                Some(browser.layout),
                Message::BrowserLayoutSelected
            ),
            row![
                text("Item size"),
                slider(20..=64, browser.item_size, Message::BrowserItemSizeChanged)
                    .width(Length::Fill),
                text(format!("{} px", browser.item_size)),
            ]
            .spacing(10),
            toggler(browser.preview_enabled)
                .label("Show preview pane")
                .on_toggle(Message::PreviewToggled),
            toggler(browser.single_click_opens_folders)
                .label("Open folders with one click")
                .on_toggle(Message::SingleClickFoldersToggled),
            pick_list(
                self.icon_theme_choices(&browser),
                Some(browser.icon_theme.clone()),
                Message::IconThemeSelected,
            )
            .placeholder("Icon theme")
            .width(Length::Fill),
            text_input("Thumbnail location", &thumbnail_location,)
                .on_input(Message::ThumbnailLocationChanged)
                .width(Length::Fill),
            pick_list(
                self.terminal_choices(),
                Some(self.selected_terminal_choice(&browser)),
                Message::TerminalChoiceSelected,
            )
            .width(Length::Fill),
            text_input(
                "Custom terminal command",
                (self.selected_terminal_choice(&browser) == CUSTOM_TERMINAL_CHOICE)
                    .then_some(browser.terminal_command.as_str())
                    .unwrap_or_default(),
            )
            .on_input(Message::TerminalCommandChanged)
            .width(Length::Fill),
            button(text("Restart backend")).on_press(Message::RestartBackend),
        ]
        .spacing(10);
        let profiles = self
            .profiles
            .iter()
            .fold(column![].spacing(6), |column, profile| {
                let selected = self.active_profile.as_deref() == Some(profile.path.as_path());
                let label = if selected {
                    format!("{} (active)", profile.name)
                } else {
                    profile.name.clone()
                };
                let profile_button = if profile.read_only {
                    button(
                        row![
                            text(label),
                            tooltip(
                                icon_text("lock").size(16),
                                text("Read-only profile"),
                                tooltip::Position::Right,
                            ),
                        ]
                        .spacing(8),
                    )
                } else {
                    button(row![text(label)].spacing(8))
                }
                .width(Length::Fill)
                .on_press(Message::SelectProfile(profile.path.clone()));
                column.push(profile_button)
            });
        let create_profile = row![
            text_input("New profile name", &self.new_profile_name)
                .on_input(Message::NewProfileNameChanged)
                .on_submit(Message::CreateProfile)
                .width(Length::Fill),
            tooltip(
                button(icon_text("plus")).on_press(Message::CreateProfile),
                text("Create profile"),
                tooltip::Position::Bottom,
            ),
        ]
        .spacing(8);
        let search_paths = self
            .config_store
            .search_paths()
            .iter()
            .fold(column![].spacing(4), |column, path| {
                column.push(text(path.display().to_string()))
            });

        let page = container(
            column![
                row![back_button, text("Preferences").size(24)].spacing(12),
                column![text("Profiles").size(18), profiles, create_profile].spacing(10),
                column![
                    row![
                        text("Color mode").size(18),
                        button(
                            row![icon_text("rotate-ccw").size(16), text("Reset profile")]
                                .spacing(6)
                        )
                        .on_press(Message::RequestProfileReset),
                    ]
                    .spacing(8),
                    options,
                    self.accent_picker_button(false),
                    self.accent_picker_button(true),
                ]
                .spacing(10),
                column![text("Browser").size(18), browser_options].spacing(10),
                column![text("Configuration search paths").size(18), search_paths].spacing(10),
            ]
            .spacing(28)
            .padding(16)
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill);
        let reset_confirmation = self.pending_profile_reset.then(|| {
            let dialog = container(
                column![
                    text("Reset active profile?"),
                    text("This restores the profile from the repository default configuration."),
                    row![
                        button(text("Cancel")).on_press(Message::CancelProfileReset),
                        button(text("Reset profile")).on_press(Message::ConfirmProfileReset),
                    ]
                    .spacing(8),
                ]
                .spacing(12),
            )
            .padding(16)
            .style(|theme: &Theme| {
                iced::widget::container::Style::default()
                    .background(theme.palette().background)
                    .border(Border {
                        color: theme.palette().primary,
                        width: 1.0,
                        radius: 6.0.into(),
                    })
            });
            stack![
                mouse_area(Space::new(Length::Fill, Length::Fill))
                    .on_press(Message::CancelProfileReset),
                container(dialog)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
        });
        let accent_picker = self.accent_picker.map(|picker| {
            let color = hsv_color(picker.hue, picker.saturation, picker.value);
            let hue_color = |hue| hsv_color(hue, picker.saturation, picker.value);
            let hue_gradient = Gradient::Linear(
                Linear::new(std::f32::consts::FRAC_PI_2)
                    .add_stop(0.0, hue_color(0))
                    .add_stop(0.17, hue_color(60))
                    .add_stop(0.33, hue_color(120))
                    .add_stop(0.5, hue_color(180))
                    .add_stop(0.67, hue_color(240))
                    .add_stop(0.83, hue_color(300))
                    .add_stop(1.0, hue_color(360)),
            );
            let hue = stack![
                container(Space::new(Length::Fill, Length::Fixed(10.0))).style(move |_| {
                    iced::widget::container::Style::default().background(hue_gradient)
                }),
                slider(0..=360, picker.hue, Message::AccentHueChanged)
                    .width(Length::Fill)
                    .style(|theme, status| {
                        let mut style = iced::widget::slider::default(theme, status);
                        style.rail.backgrounds = (
                            Background::Color(Color::TRANSPARENT),
                            Background::Color(Color::TRANSPARENT),
                        );
                        style
                    }),
            ]
            .width(Length::Fill)
            .height(Length::Fixed(24.0));
            let saturation_gradient = Gradient::Linear(
                Linear::new(std::f32::consts::FRAC_PI_2)
                    .add_stop(0.0, hsv_color(picker.hue, 0, picker.value))
                    .add_stop(1.0, hsv_color(picker.hue, 255, picker.value)),
            );
            let saturation = stack![
                container(Space::new(Length::Fill, Length::Fixed(10.0))).style(move |_| {
                    iced::widget::container::Style::default().background(saturation_gradient)
                }),
                slider(0..=255, picker.saturation, Message::AccentSaturationChanged)
                    .width(Length::Fill)
                    .style(|theme, status| {
                        let mut style = iced::widget::slider::default(theme, status);
                        style.rail.backgrounds = (
                            Background::Color(Color::TRANSPARENT),
                            Background::Color(Color::TRANSPARENT),
                        );
                        style
                    }),
            ]
            .width(Length::Fill)
            .height(Length::Fixed(24.0));
            let value_gradient = Gradient::Linear(
                Linear::new(std::f32::consts::FRAC_PI_2)
                    .add_stop(0.0, Color::BLACK)
                    .add_stop(1.0, hsv_color(picker.hue, picker.saturation, 255)),
            );
            let value = stack![
                container(Space::new(Length::Fill, Length::Fixed(10.0))).style(move |_| {
                    iced::widget::container::Style::default().background(value_gradient)
                }),
                slider(0..=255, picker.value, Message::AccentValueChanged)
                    .width(Length::Fill)
                    .style(|theme, status| {
                        let mut style = iced::widget::slider::default(theme, status);
                        style.rail.backgrounds = (
                            Background::Color(Color::TRANSPARENT),
                            Background::Color(Color::TRANSPARENT),
                        );
                        style
                    }),
            ]
            .width(Length::Fill)
            .height(Length::Fixed(24.0));
            let dialog = container(
                column![
                    text(if picker.dark {
                        "Dark accent color"
                    } else {
                        "Light accent color"
                    }),
                    container(Space::new(Length::Fill, Length::Fixed(42.0))).style(move |_| {
                        iced::widget::container::Style::default().background(color)
                    }),
                    text("Hue"),
                    hue,
                    text("Saturation"),
                    saturation,
                    text("Value"),
                    value,
                    row![
                        button(text("Cancel")).on_press(Message::CancelAccentPicker),
                        button(text("Apply")).on_press(Message::ConfirmAccentPicker),
                    ]
                    .spacing(8),
                ]
                .spacing(12),
            )
            .width(Length::Fixed(320.0))
            .padding(16)
            .style(|theme: &Theme| {
                iced::widget::container::Style::default()
                    .background(theme.palette().background)
                    .border(Border {
                        color: theme.palette().primary,
                        width: 1.0,
                        radius: 6.0.into(),
                    })
            });
            stack![
                mouse_area(Space::new(Length::Fill, Length::Fill))
                    .on_press(Message::CancelAccentPicker),
                container(dialog)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
        });
        stack![page]
            .push_maybe(reset_confirmation)
            .push_maybe(accent_picker)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

fn icon_text<'a>(name: &str) -> iced::widget::Text<'a> {
    let icon = try_icon(Pack::Lucide, name, Style::Regular, Size::Regular)
        .expect("missing bundled Lucide icon");
    let glyph = char::from_u32(icon.codepoint).unwrap_or('?');

    text(glyph.to_string())
        .size(18)
        .font(Font::with_name(icon.family))
}

fn sort_entries(entries: &mut [proto::FileEntry]) {
    entries.sort_by_key(|entry| {
        (
            !entry.is_directory,
            entry.name.starts_with('.'),
            entry.name.to_lowercase(),
        )
    });
}

fn available_icon_themes() -> Vec<String> {
    let mut themes = vec!["bundled".into()];
    #[cfg(target_os = "linux")]
    {
        let theme = Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "icon-theme"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .trim_matches('\'')
                    .to_owned()
            })
            .filter(|theme| !theme.is_empty());
        if let Some(theme) = theme
            && !themes.contains(&theme)
        {
            themes.push(theme);
        }
    }
    themes
}

fn themed_entry_icon_path(theme: &str, entry: &proto::FileEntry) -> Option<PathBuf> {
    if theme == "bundled" {
        return None;
    }
    let icons = gio_icon_names(Path::new(&entry.path));
    for root in icon_theme_directories(theme) {
        for size in ["128x128", "96x96", "64x64", "48x48", "scalable"] {
            for icon in &icons {
                for category in ["mimetypes", "places", "actions", "status", "apps"] {
                    let path = root.join(size).join(category).join(format!("{icon}.svg"));
                    if path.is_file() {
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}

fn gio_icon_names(path: &Path) -> Vec<String> {
    let Ok(output) = Command::new("gio")
        .args(["info", "-a", "standard::icon"])
        .arg(path)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.trim().strip_prefix("standard::icon:"))
        .map(|icons| {
            icons
                .split(',')
                .map(str::trim)
                .filter(|icon| !icon.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn icon_theme_directories(theme: &str) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        directories.push(home.join(".nix-profile/share/icons").join(theme));
        directories.push(home.join(".local/share/icons").join(theme));
    }
    directories.push(PathBuf::from("/run/current-system/sw/share/icons").join(theme));
    directories
}

fn file_item_button_style(theme: &Theme, status: button::Status, selected: bool) -> button::Style {
    let base = button::text(theme, status);
    let style = button::Style {
        border: Border {
            radius: 6.0.into(),
            ..base.border
        },
        ..base
    };
    if selected {
        button::Style {
            text_color: theme.palette().background,
            ..style.with_background(theme.palette().primary)
        }
    } else if matches!(status, button::Status::Hovered) {
        style.with_background(Color::from_rgba8(128, 128, 128, 0.18))
    } else {
        style
    }
}

fn context_menu_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let style = button::Style {
        border: Border {
            radius: 4.0.into(),
            ..button::text(theme, status).border
        },
        ..button::text(theme, status)
    };
    if matches!(status, button::Status::Hovered) {
        style.with_background(Color::from_rgba8(128, 128, 128, 0.18))
    } else {
        style
    }
}

fn sidebar_icon(location: &SidebarLocation) -> &'static str {
    match location.label.as_str() {
        "Home" => "house",
        "Downloads" => "download",
        "Pictures" => "image",
        _ => "folder",
    }
}

fn parse_color(value: &str) -> Option<Color> {
    let value = value.strip_prefix('#')?;
    if value.len() != 6 {
        return None;
    }
    let red = u8::from_str_radix(&value[0..2], 16).ok()?;
    let green = u8::from_str_radix(&value[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&value[4..6], 16).ok()?;
    Some(Color::from_rgb8(red, green, blue))
}

fn hsv_color(hue: u16, saturation: u8, value: u8) -> Color {
    let hue = (hue % 360) as f32 / 60.0;
    let value = f32::from(value) / 255.0;
    let chroma = value * f32::from(saturation) / 255.0;
    let secondary = chroma * (1.0 - ((hue % 2.0) - 1.0).abs());
    let (red, green, blue) = match hue as u8 {
        0 => (chroma, secondary, 0.0),
        1 => (secondary, chroma, 0.0),
        2 => (0.0, chroma, secondary),
        3 => (0.0, secondary, chroma),
        4 => (secondary, 0.0, chroma),
        _ => (chroma, 0.0, secondary),
    };
    let offset = value - chroma;
    Color::from_rgb(red + offset, green + offset, blue + offset)
}

fn rgb_to_hsv(color: Color) -> (u16, u8, u8) {
    let maximum = color.r.max(color.g).max(color.b);
    let minimum = color.r.min(color.g).min(color.b);
    let delta = maximum - minimum;
    let hue = if delta == 0.0 {
        0.0
    } else if maximum == color.r {
        60.0 * ((color.g - color.b) / delta).rem_euclid(6.0)
    } else if maximum == color.g {
        60.0 * ((color.b - color.r) / delta + 2.0)
    } else {
        60.0 * ((color.r - color.g) / delta + 4.0)
    };
    let saturation = if maximum == 0.0 { 0.0 } else { delta / maximum };
    (
        hue.round() as u16,
        (saturation * 255.0).round() as u8,
        (maximum * 255.0).round() as u8,
    )
}

#[derive(Debug, Deserialize)]
struct LsblkOutput {
    #[serde(default)]
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Debug, Deserialize)]
struct LsblkDevice {
    name: String,
    path: Option<PathBuf>,
    label: Option<String>,
    #[serde(default)]
    mountpoints: Vec<Option<PathBuf>>,
    #[serde(default)]
    children: Vec<LsblkDevice>,
    #[serde(rename = "type")]
    device_type: String,
}

async fn load_mounts() -> Result<MountState, String> {
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("lsblk")
            .args(["--json", "--output", "NAME,PATH,LABEL,MOUNTPOINTS,TYPE"])
            .output()
            .map_err(|error| format!("Could not list drives: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        let output: LsblkOutput = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("Could not read drive list: {error}"))?;
        let mut drives = Vec::new();
        for device in output.blockdevices {
            collect_drives(device, &mut drives);
        }
        Ok(MountState {
            drives,
            mounts: read_mounts()?,
        })
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(MountState {
            drives: Vec::new(),
            mounts: Vec::new(),
        })
    }
}

#[cfg(target_os = "linux")]
fn collect_drives(device: LsblkDevice, drives: &mut Vec<Drive>) {
    let is_volume = matches!(device.device_type.as_str(), "disk" | "part")
        && (device.device_type == "part" || device.children.is_empty());
    if is_volume {
        if let Some(path) = device.path {
            let mount_points = device.mountpoints.into_iter().flatten().collect();
            drives.push(Drive {
                path,
                name: device.label.unwrap_or(device.name),
                mount_points,
            });
        }
    }
    for child in device.children {
        collect_drives(child, drives);
    }
}

async fn mount_drive(path: PathBuf) -> Result<MountState, String> {
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("udisksctl")
            .args(["mount", "--block"])
            .arg(&path)
            .output()
            .map_err(|error| format!("Could not mount {}: {error}", path.display()))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        load_mounts().await
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err("Mounting drives is not supported on this platform".into())
    }
}

async fn open_file(path: PathBuf) -> Result<(), String> {
    let status = Command::new("xdg-open")
        .arg(&path)
        .status()
        .map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("xdg-open could not open {}", path.display()))
    }
}

async fn open_terminal(path: PathBuf, configured_command: String) -> Result<(), String> {
    let command = configured_command.trim();
    if !command.is_empty() && command != "default" {
        return spawn_terminal_command(command, &path);
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(command) = gnome_default_terminal_command()
            .filter(|command| terminal_command_is_available(command))
        {
            return spawn_terminal_command(&command, &path);
        }
        for command in ["xdg-terminal-exec", "x-terminal-emulator"]
            .into_iter()
            .chain(RECOMMENDED_TERMINALS.iter().copied())
        {
            if terminal_command_is_available(command) {
                return spawn_terminal_command(command, &path);
            }
        }
        Err(format!(
            "No supported terminal command is available for {}",
            path.display()
        ))
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .args(["-a", "Terminal"])
            .arg(&path)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Could not open Terminal in {}: {error}", path.display()))
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", "cmd", "/K"])
            .current_dir(&path)
            .spawn()
            .map(|_| ())
            .map_err(|error| {
                format!(
                    "Could not open Command Prompt in {}: {error}",
                    path.display()
                )
            })
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = path;
        Err("Opening a terminal is not supported on this platform".into())
    }
}

#[cfg(target_os = "linux")]
fn recommended_terminal_commands() -> Vec<String> {
    let script = format!(
        "for app in {}; do command -v \"$app\" 2>/dev/null; done",
        RECOMMENDED_TERMINALS.join(" ")
    );
    let Ok(output) = Command::new("sh").args(["-c", &script]).output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

#[cfg(not(target_os = "linux"))]
fn recommended_terminal_commands() -> Vec<String> {
    Vec::new()
}

fn spawn_terminal_command(command: &str, path: &Path) -> Result<(), String> {
    Command::new("sh")
        .args(["-c", command])
        .current_dir(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open {command} in {}: {error}", path.display()))
}

#[cfg(target_os = "linux")]
fn gnome_default_terminal_command() -> Option<String> {
    let output = Command::new("gsettings")
        .args([
            "get",
            "org.gnome.desktop.default-applications.terminal",
            "exec",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let command = String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_matches('\'')
        .to_owned();
    (!command.is_empty()).then_some(command)
}

#[cfg(target_os = "linux")]
fn terminal_command_is_available(command: &str) -> bool {
    let Some(executable) = command.split_whitespace().next() else {
        return false;
    };
    let executable_path = Path::new(executable);
    if executable_path.components().count() > 1 {
        return executable_path.is_file();
    }
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| directory.join(executable).is_file())
    })
}

async fn default_file_opener(path: PathBuf) -> Result<String, String> {
    let mime_output = Command::new("file")
        .args(["--mime-type", "-b"])
        .arg(&path)
        .output()
        .map_err(|error| {
            format!(
                "Could not determine the type of {}: {error}",
                path.display()
            )
        })?;
    if !mime_output.status.success() {
        return Err(format!(
            "Could not determine the type of {}",
            path.display()
        ));
    }
    let mime = String::from_utf8_lossy(&mime_output.stdout)
        .trim()
        .to_owned();
    if mime.is_empty() {
        return Err(format!("No MIME type was returned for {}", path.display()));
    }

    let application_output = Command::new("xdg-mime")
        .args(["query", "default", &mime])
        .output()
        .map_err(|error| format!("Could not find an application for {mime}: {error}"))?;
    if !application_output.status.success() {
        return Err(format!("Could not find an application for {mime}"));
    }
    let application = String::from_utf8_lossy(&application_output.stdout)
        .trim()
        .to_owned();
    if application.is_empty() {
        Err(format!("No default application is configured for {mime}"))
    } else {
        Ok(desktop_entry_name(&application).unwrap_or(application))
    }
}

fn desktop_entry_name(application: &str) -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let user = std::env::var_os("USER")?;
    let output = Command::new("find")
        .arg(PathBuf::from(&home).join(".local/share/applications"))
        .arg(PathBuf::from(&home).join(".nix-profile/share/applications"))
        .arg(
            PathBuf::from("/etc/profiles/per-user")
                .join(user)
                .join("share/applications"),
        )
        .arg("/run/current-system/sw/share/applications")
        .args([
            "-name",
            application,
            "-exec",
            "awk",
            "-F=",
            "/^Name=/{print substr($0,6); exit}",
            "{}",
            ";",
            "-quit",
        ])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!name.is_empty()).then_some(name)
}

#[cfg(target_os = "linux")]
fn read_mounts() -> Result<Vec<SystemMount>, String> {
    let mountinfo = fs::read_to_string("/proc/self/mountinfo")
        .map_err(|error| format!("Could not read mounted filesystems: {error}"))?;
    Ok(mountinfo
        .lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let mount_path = left.split_whitespace().nth(4)?;
            let mut filesystem_fields = right.split_whitespace();
            let filesystem = filesystem_fields.next()?;
            let _source = filesystem_fields.next()?;
            let mount = SystemMount {
                path: PathBuf::from(unescape_mount_path(mount_path)),
                filesystem: filesystem.into(),
            };
            (!is_system_mount(&mount)).then_some(mount)
        })
        .collect())
}

#[cfg(target_os = "linux")]
fn is_system_mount(mount: &SystemMount) -> bool {
    const SYSTEM_FILESYSTEMS: &[&str] = &[
        "proc",
        "sysfs",
        "devtmpfs",
        "devpts",
        "tmpfs",
        "cgroup",
        "cgroup2",
        "securityfs",
        "pstore",
        "tracefs",
        "configfs",
        "debugfs",
        "mqueue",
        "hugetlbfs",
        "fusectl",
    ];
    SYSTEM_FILESYSTEMS.contains(&mount.filesystem.as_str())
        || ["/proc", "/sys", "/dev", "/run"]
            .iter()
            .any(|path| mount.path.starts_with(path))
}

#[cfg(target_os = "linux")]
fn unescape_mount_path(path: &str) -> String {
    path.replace(r"\040", " ")
        .replace(r"\011", "\t")
        .replace(r"\012", "\n")
        .replace(r"\134", r"\")
}
