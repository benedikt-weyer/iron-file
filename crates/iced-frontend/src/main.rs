use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use iced::{
    Border, Color, Element, Font, Length, Point, Task, Theme, mouse,
    widget::{
        Space, button, column, container, image, mouse_area, pick_list, radio, responsive, row,
        scrollable, slider, stack, svg, text, text_input, toggler, tooltip,
    },
};
use iconflow::{Pack, Size, Style, fonts, try_icon};
use iron_file_common::{
    browse_with_thumbnails,
    config::{BrowserLayout, BrowserSettings, ColorMode, ConfigStore, Profile, SidebarLocation},
    create_thumbnail, ensure_backend, pipe_backend_logs, proto,
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
    context_entry: Option<ContextEntry>,
    pointer_position: Point,
    context_position: Point,
    dragging_sidebar_location: Option<PathBuf>,
    last_entry_click: Option<(PathBuf, Instant)>,
    terminal_recommendations: Vec<String>,
    history: Vec<PathBuf>,
    history_index: Option<usize>,
    sidebar_resize: Option<(f32, u16, u16)>,
    icon_themes: Vec<String>,
    entry_icons: HashMap<PathBuf, Option<PathBuf>>,
    thumbnail_handles: HashMap<PathBuf, image::Handle>,
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
    OpenParent,
    ShowBrowser,
    ShowPreferences,
    SelectProfile(PathBuf),
    NewProfileNameChanged(String),
    CreateProfile,
    ResetActiveProfile,
    ColorModeSelected(ColorMode),
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
    MountsLoaded(Result<MountState, String>),
    MountDrive(PathBuf),
    FileOpened(Result<(), String>),
    TerminalOpened(Result<(), String>),
    BackendLogPipeEnded(Result<(), String>),
    ThumbnailGenerated {
        path: PathBuf,
        thumbnail_path: Result<String, String>,
    },
    BrowseFinished {
        result: Result<BrowseResponse, String>,
        history: HistoryRequest,
    },
    IconFontLoaded(Result<(), iced::font::Error>),
}

impl Gui {
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
            context_entry: None,
            pointer_position: Point::ORIGIN,
            context_position: Point::ORIGIN,
            dragging_sidebar_location: None,
            last_entry_click: None,
            terminal_recommendations: recommended_terminal_commands(),
            history: Vec::new(),
            history_index: None,
            sidebar_resize: None,
            icon_themes: available_icon_themes(),
            entry_icons: HashMap::new(),
            thumbnail_handles: HashMap::new(),
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
            Message::NavigateBack => self.navigate_history(-1),
            Message::NavigateForward => self.navigate_history(1),
            Message::EntryClicked { path, is_directory } => {
                self.handle_entry_click(path, is_directory)
            }
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
            Message::ResetActiveProfile => {
                self.reset_active_profile();
                Task::none()
            }
            Message::ColorModeSelected(color_mode) => {
                self.save_color_mode(color_mode);
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
                Task::none()
            }
            Message::SidebarReleased(path) => self.release_sidebar_location(path),
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
                self.entries = directory.entries;
                self.thumbnail_handles.clear();
                self.refresh_entry_icons();
                self.content.clear();
                self.status = format!("{} entries", self.entries.len());
                let thumbnail_directory = self.active_browser_settings().thumbnail_location;
                Task::batch(
                    self.entries
                        .iter()
                        .filter(|entry| !entry.is_directory)
                        .map(|entry| {
                            let path = PathBuf::from(&entry.path);
                            Task::perform(
                                create_thumbnail(path.clone(), thumbnail_directory.clone()),
                                move |thumbnail_path| Message::ThumbnailGenerated {
                                    path: path.clone(),
                                    thumbnail_path,
                                },
                            )
                        }),
                )
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
            if entry.is_directory {
                column.push(
                    mouse_area(
                        button(row![icon, text(&entry.name)].spacing(8))
                            .style(file_item_button_style)
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
                            .style(file_item_button_style)
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
            responsive(move |size| {
                let tile_width = f32::from(browser_settings.item_size) * 3.5;
                let tile_height = tile_width * 1.2;
                let columns = (size.width / tile_width).floor().max(1.0) as usize;
                let tiles =
                    self.entries
                        .chunks(columns)
                        .fold(column![].spacing(8), |column, chunk| {
                            let tiles = chunk.iter().fold(row![].spacing(8), |row, entry| {
                                let path = PathBuf::from(&entry.path);
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
                                    .style(file_item_button_style)
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
        let browser = mouse_area(browser).on_right_press(Message::ShowEntryContext {
            path: self.directory_path.clone(),
            is_directory: true,
        });
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
                    button(text("Remove from sidebar"))
                        .width(Length::Fill)
                        .on_press(Message::RemoveContextFolderFromSidebar)
                        .into()
                } else {
                    button(text("Add to sidebar"))
                        .width(Length::Fill)
                        .on_press(Message::AddContextFolderToSidebar)
                        .into()
                }
            } else {
                match &entry.opener {
                    Some(Ok(application)) => button(text(format!("Open (with {application})")))
                        .width(Length::Fill)
                        .on_press(Message::OpenContextFile)
                        .into(),
                    Some(Err(_)) => button(text("Open"))
                        .width(Length::Fill)
                        .on_press(Message::OpenContextFile)
                        .into(),
                    None => text("Finding default application...").into(),
                }
            };
            let mut actions = column![action].spacing(4);
            if entry.is_directory {
                actions = actions.push(
                    button(text("Open terminal here"))
                        .width(Length::Fill)
                        .on_press(Message::OpenTerminalHere),
                );
            }
            let menu = container(actions).padding(8);
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

        stack![page]
            .push_maybe(overlay)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn entry_icon<'a>(&self, entry: &proto::FileEntry, size: u16) -> Element<'a, Message> {
        if let Some(handle) = self.thumbnail_handles.get(&PathBuf::from(&entry.path)) {
            image(handle.clone())
                .width(Length::Fixed(f32::from(size)))
                .height(Length::Fixed(f32::from(size)))
                .into()
        } else if let Some(path) = self
            .entry_icons
            .get(&PathBuf::from(&entry.path))
            .and_then(|path| path.as_ref())
        {
            svg(svg::Handle::from_path(path))
                .width(Length::Fixed(f32::from(size)))
                .height(Length::Fixed(f32::from(size)))
                .into()
        } else {
            icon_text(if entry.is_directory { "folder" } else { "file" })
                .size(size)
                .into()
        }
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
                column.push(
                    mouse_area(item)
                        .on_press(Message::SidebarPressed(location.path.clone()))
                        .on_release(Message::SidebarReleased(location.path)),
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
        let sidebar_content = column![locations, mounts].spacing(20);
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

        container(
            column![
                row![back_button, text("Preferences").size(24)].spacing(12),
                column![text("Profiles").size(18), profiles, create_profile].spacing(10),
                column![
                    row![
                        text("Color mode").size(18),
                        tooltip(
                            button(icon_text("rotate-ccw")).on_press(Message::ResetActiveProfile),
                            text("Reset active profile to defaults"),
                            tooltip::Position::Bottom,
                        ),
                    ]
                    .spacing(8),
                    options,
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

fn file_item_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let base = button::text(theme, status);
    let style = button::Style {
        border: Border {
            radius: 6.0.into(),
            ..base.border
        },
        ..base
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
