mod backdrop_blur;
mod browser_view;
mod preferences_view;
mod utilities;

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
    keyboard, mouse,
    widget::{
        Space, button as button_style, checkbox, container, image, mouse_area, opaque, pick_list,
        radio, responsive, row, scrollable, slider, svg, text, text_input, toggler, tooltip,
    },
    window,
};
use iconflow::{Pack, Size, Style, fonts, try_icon};
use iron_file_common::{
    browse_with_thumbnails, compress_entries,
    config::{
        BrowserLayout, BrowserSettings, ColorMode, ConfigStore, ContextMenuBlurKernelSize,
        ContextMenuItem, EntrySortOrder, FolderSortOverride, KeyboardShortcutAction, Profile,
        QuickToolbarItem, SidebarLocation,
    },
    copy_entries, create_entry, create_symlinks, create_thumbnail, delete_entries, ensure_backend,
    extract_archives, inspect_entry, pipe_backend_logs, proto, rename_entry, restart_backend,
    stream_directory,
};
use proto::{BrowseResponse, browse_response::Payload};
use serde::Deserialize;
use tokio::runtime::Runtime;

const DETACHED_ENV: &str = "IRON_FILE_DETACHED";
const NAVIGATION_CONTROL_HEIGHT: f32 = 32.0;
static BORDER_RADIUS: AtomicU8 = AtomicU8::new(6);

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

    prefer_x11_when_available();
    if let Ok(runtime) = Runtime::new() {
        let _ = runtime.block_on(ensure_backend());
    }
    iced::application("Iron File", Gui::update, Gui::view)
        .theme(Gui::theme)
        .subscription(Gui::subscription)
        .window(window::Settings {
            transparent: true,
            platform_specific: window::settings::PlatformSpecific {
                // Must match iron-file.desktop so the desktop shell can resolve the dock icon.
                application_id: "iron-file".into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .run_with(move || {
            let gui = Gui::new(startup.follow_logs, startup.initial_path, startup.picker);
            let task = gui.load_initial_directory();
            (gui, task)
        })?;
    Ok(())
}

struct StartupOptions {
    follow_logs: bool,
    initial_path: Option<PathBuf>,
    picker: Option<PickerOptions>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerKind {
    File,
    Folder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PickerOptions {
    kind: PickerKind,
    multiple: bool,
    save_file_name: Option<String>,
}

fn startup_options() -> StartupOptions {
    parse_startup_options(env::args_os().skip(1))
}

fn parse_startup_options(arguments: impl IntoIterator<Item = OsString>) -> StartupOptions {
    let mut options = StartupOptions {
        follow_logs: false,
        initial_path: None,
        picker: None,
    };
    let mut picker_kind = PickerKind::File;
    let mut picker_multiple = false;
    let mut picker_requested = false;
    let mut save_file_name = None;

    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "-f" || argument == "--follow" {
            options.follow_logs = true;
        } else if argument == "--mode" {
            picker_requested = arguments.next().is_some_and(|mode| mode == "picker");
        } else if argument == "--picker" {
            picker_requested = true;
        } else if argument == "--file" {
            picker_kind = PickerKind::File;
        } else if argument == "--folder" {
            picker_kind = PickerKind::Folder;
        } else if argument == "--multiple" || argument == "--multi" {
            picker_multiple = true;
        } else if argument == "--single" {
            picker_multiple = false;
        } else if argument == "--save-name" {
            save_file_name = arguments.next().and_then(|name| name.into_string().ok());
        } else if !argument.to_string_lossy().starts_with('-') && options.initial_path.is_none() {
            options.initial_path = Some(PathBuf::from(argument));
        }
    }

    options.picker = picker_requested.then_some(PickerOptions {
        kind: picker_kind,
        multiple: picker_multiple,
        save_file_name,
    });

    options
}

fn resolve_initial_path(path: PathBuf) -> PathBuf {
    let current_directory = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    resolve_path_from(path, current_directory)
}

fn resolve_path_from(path: PathBuf, current_directory: PathBuf) -> PathBuf {
    let path = if path.is_absolute() {
        path
    } else {
        current_directory.join(path)
    };
    fs::canonicalize(&path).unwrap_or(path)
}

fn valid_save_file_name(name: &str) -> bool {
    !name.is_empty()
        && Path::new(name)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
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

fn is_detached() -> bool {
    env::var_os(DETACHED_ENV).is_some()
}

fn detach() -> std::io::Result<()> {
    Command::new(env::current_exe()?)
        .args(env::args_os().skip(1))
        .env(DETACHED_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod startup_tests {
    use super::*;

    #[test]
    fn accepts_a_positional_location_and_follow_flag() {
        let options =
            parse_startup_options(["-f", "/tmp/first", "/tmp/second"].map(OsString::from));

        assert!(options.follow_logs);
        assert_eq!(options.initial_path, Some(PathBuf::from("/tmp/first")));
    }

    #[test]
    fn accepts_a_location_before_the_follow_flag() {
        let options = parse_startup_options(["/tmp/first", "--follow"].map(OsString::from));

        assert!(options.follow_logs);
        assert_eq!(options.initial_path, Some(PathBuf::from("/tmp/first")));
    }

    #[test]
    fn parses_folder_multi_picker_mode() {
        let options = parse_startup_options(
            ["--mode", "picker", "--folder", "--multiple"].map(OsString::from),
        );

        assert_eq!(
            options.picker,
            Some(PickerOptions {
                kind: PickerKind::Folder,
                multiple: true,
                save_file_name: None,
            })
        );
    }

    #[test]
    fn parses_a_save_name_for_picker_mode() {
        let options = parse_startup_options(
            ["--mode", "picker", "--folder", "--save-name", "report.txt"].map(OsString::from),
        );

        assert_eq!(
            options.picker.and_then(|picker| picker.save_file_name),
            Some("report.txt".into())
        );
    }

    #[test]
    fn save_name_cannot_escape_the_selected_folder() {
        assert!(valid_save_file_name("report.txt"));
        assert!(!valid_save_file_name("../report.txt"));
        assert!(!valid_save_file_name("/tmp/report.txt"));
    }

    #[test]
    fn accepts_current_and_relative_locations() {
        let current = parse_startup_options(["."].map(OsString::from));
        let relative = parse_startup_options(["./Documents"].map(OsString::from));

        assert_eq!(current.initial_path, Some(PathBuf::from(".")));
        assert_eq!(relative.initial_path, Some(PathBuf::from("./Documents")));
    }

    #[test]
    fn resolves_relative_locations_from_the_invoking_directory() {
        let location = resolve_path_from(PathBuf::from("."), PathBuf::from("/tmp"));

        assert_eq!(location, PathBuf::from("/tmp"));
    }
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
    follow_logs: bool,
    picker: Option<PickerOptions>,
    save_file_name: Option<String>,
    original_save_file_name: Option<String>,
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
    pending_info: Option<InfoDialog>,
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
    pending_rename: Option<PathBuf>,
    rename_entry_name: String,
    pending_profile_reset: bool,
    pending_compression: bool,
    compression_level: u8,
    compression_type: ArchiveCompression,
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
enum InfoDialog {
    Loading(PathBuf),
    Loaded(EntryInfo),
    Error { path: PathBuf, error: String },
}

#[derive(Debug, Clone)]
struct EntryInfo {
    path: PathBuf,
    name: String,
    rows: Vec<(String, String)>,
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
    RenameSelection,
    CopyLocation(PathBuf),
    Paste,
    DeleteSelection,
    AddSymlinkToPasteBuffer(PathBuf),
    CreateSymlinksHere(PathBuf),
    CompressSelection,
    ExtractSelection,
}

#[derive(Debug, Clone, Copy)]
enum SelectionDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FolderSortSelection {
    None,
    NameAscending,
    NameDescending,
    ModifiedNewest,
    ModifiedOldest,
    CreatedNewest,
    CreatedOldest,
}

impl FolderSortSelection {
    const ALL: [Self; 7] = [
        Self::None,
        Self::NameAscending,
        Self::NameDescending,
        Self::ModifiedNewest,
        Self::ModifiedOldest,
        Self::CreatedNewest,
        Self::CreatedOldest,
    ];

    fn sort_order(self) -> Option<EntrySortOrder> {
        match self {
            Self::None => None,
            Self::NameAscending => Some(EntrySortOrder::NameAscending),
            Self::NameDescending => Some(EntrySortOrder::NameDescending),
            Self::ModifiedNewest => Some(EntrySortOrder::ModifiedNewest),
            Self::ModifiedOldest => Some(EntrySortOrder::ModifiedOldest),
            Self::CreatedNewest => Some(EntrySortOrder::CreatedNewest),
            Self::CreatedOldest => Some(EntrySortOrder::CreatedOldest),
        }
    }
}

impl From<Option<EntrySortOrder>> for FolderSortSelection {
    fn from(order: Option<EntrySortOrder>) -> Self {
        match order {
            None => Self::None,
            Some(EntrySortOrder::NameAscending) => Self::NameAscending,
            Some(EntrySortOrder::NameDescending) => Self::NameDescending,
            Some(EntrySortOrder::ModifiedNewest) => Self::ModifiedNewest,
            Some(EntrySortOrder::ModifiedOldest) => Self::ModifiedOldest,
            Some(EntrySortOrder::CreatedNewest) => Self::CreatedNewest,
            Some(EntrySortOrder::CreatedOldest) => Self::CreatedOldest,
        }
    }
}

impl std::fmt::Display for FolderSortSelection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::None => "Folder: None",
            Self::NameAscending => "Folder: Name (A-Z)",
            Self::NameDescending => "Folder: Name (Z-A)",
            Self::ModifiedNewest => "Folder: Last modified (newest)",
            Self::ModifiedOldest => "Folder: Last modified (oldest)",
            Self::CreatedNewest => "Folder: Created (newest)",
            Self::CreatedOldest => "Folder: Created (oldest)",
        })
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveCompression {
    Store,
    Deflate,
    Bzip2,
    Zstd,
}

impl ArchiveCompression {
    fn value(self) -> &'static str {
        match self {
            Self::Store => "store",
            Self::Deflate => "deflate",
            Self::Bzip2 => "bzip2",
            Self::Zstd => "zstd",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum PreferenceOption {
    ColorMode,
    LightAccent,
    DarkAccent,
    BackgroundOpacity,
    ContextMenuBlurStrength,
    ContextMenuBlurKernelSize,
    FileContextMenuItems,
    FolderContextMenuItems,
    QuickToolbarItems,
    KeyboardShortcuts,
    BorderRadius,
    Layout,
    SmoothScrolling,
    ItemSize,
    MaxNameLines,
    Preview,
    SingleClickFolders,
    IconTheme,
    ThumbnailLocation,
    Terminal,
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
    EscapePressed,
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
    ArchiveFinished {
        action: &'static str,
        result: Result<Vec<PathBuf>, String>,
    },
    CompressionLevelChanged(u8),
    CompressionTypeSelected(ArchiveCompression),
    ConfirmCompression,
    CancelCompression,
    ConfirmDelete,
    CancelDelete,
    SelectDeleteDialogAction(bool),
    ActivateDeleteDialogAction,
    ArrowKeyPressed(SelectionDirection),
    FileDeleteFinished(Result<Vec<PathBuf>, String>),
    RequestCreateEntry {
        parent: PathBuf,
        is_directory: bool,
    },
    CreateEntryNameChanged(String),
    ConfirmCreateEntry,
    CancelCreateEntry,
    EntryCreated(Result<PathBuf, String>),
    RequestRenameEntry(PathBuf),
    RenameEntryNameChanged(String),
    ConfirmRenameEntry,
    CancelRenameEntry,
    EntryRenamed(Result<PathBuf, String>),
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
    ResetPreference(PreferenceOption),
    ColorModeSelected(ColorMode),
    BackgroundOpacityChanged(u8),
    ContextMenuBlurStrengthChanged(u8),
    ContextMenuBlurKernelSizeChanged(ContextMenuBlurKernelSize),
    ContextMenuItemToggled {
        item: ContextMenuItem,
        is_directory: bool,
        enabled: bool,
    },
    MoveContextMenuItem {
        item: ContextMenuItem,
        is_directory: bool,
        move_up: bool,
    },
    QuickToolbarItemToggled(QuickToolbarItem, bool),
    MoveQuickToolbarItem(QuickToolbarItem, bool),
    SortOrderSelected(EntrySortOrder),
    FolderSortOverrideSelected(FolderSortSelection),
    KeyboardShortcutChanged {
        action: KeyboardShortcutAction,
        key: String,
    },
    ShortcutPressed(String),
    BorderRadiusChanged(u8),
    OpenAccentPicker(bool),
    AccentHueChanged(u16),
    AccentSaturationChanged(u8),
    AccentValueChanged(u8),
    ConfirmAccentPicker,
    CancelAccentPicker,
    BrowserLayoutSelected(BrowserLayout),
    SmoothScrollingToggled(bool),
    BrowserItemSizeChanged(u16),
    MaxNameLinesChanged(u8),
    PreviewToggled(bool),
    ToggleHiddenFiles,
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
    RequestEntryInfo(PathBuf),
    EntryInfoLoaded {
        path: PathBuf,
        result: Result<proto::EntryInfoResponse, String>,
    },
    CloseEntryInfo,
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
    ConfirmPicker,
    CancelPicker,
    SaveFileNameChanged(String),
    ResetSaveFileName,
    CloseWindow(Option<window::Id>),
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
                    keyboard::Key::Character("v" | "V") => {
                        Some(Message::ExecuteBrowserCommand(BrowserCommand::Paste))
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
        let directory_path = initial_path
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
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
            pending_info: None,
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
            pending_rename: None,
            rename_entry_name: String::new(),
            pending_profile_reset: false,
            pending_compression: false,
            compression_level: 6,
            compression_type: ArchiveCompression::Deflate,
            selection_anchor: None,
            modifiers: keyboard::Modifiers::default(),
            browser_pointer: Point::ORIGIN,
            rectangle_selection: None,
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
                if self.pending_info.is_some() {
                    self.pending_info = None;
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
                Task::none()
            }
            Message::NavigateBack => self.navigate_history(-1),
            Message::NavigateForward => self.navigate_history(1),
            Message::EntryClicked { path, is_directory } => {
                self.handle_entry_click(path, is_directory)
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
            Message::CloseWindow(Some(id)) => window::close(id),
            Message::CloseWindow(None) => Task::none(),
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
            Message::RequestRenameEntry(path) => {
                self.context_entry = None;
                self.rename_entry_name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.pending_rename = Some(path);
                Task::none()
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
            Message::SmoothScrollingToggled(smooth_scrolling) => {
                let mut browser = self.active_browser_settings();
                browser.smooth_scrolling = smooth_scrolling;
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
                let sort_order = self.current_sort_order();
                self.entries.push(entry);
                sort_entries(&mut self.entries, sort_order);
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
        palette.background = palette
            .background
            .scale_alpha(f32::from(theme_settings.background_opacity.min(100)) / 100.0);
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
            PreferenceOption::SmoothScrolling => {
                browser.smooth_scrolling == browser_defaults.smooth_scrolling
            }
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
        window::get_oldest().map(Message::CloseWindow)
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

    fn reset_preference(&mut self, option: PreferenceOption) {
        let defaults = iron_file_common::config::default_browser_settings();
        match option {
            PreferenceOption::ColorMode => self.save_color_mode(ColorMode::default()),
            PreferenceOption::LightAccent
            | PreferenceOption::DarkAccent
            | PreferenceOption::BackgroundOpacity
            | PreferenceOption::ContextMenuBlurStrength
            | PreferenceOption::ContextMenuBlurKernelSize
            | PreferenceOption::BorderRadius => {
                let defaults = iron_file_common::config::default_theme_settings();
                let mut theme = self.active_theme_settings();
                if matches!(option, PreferenceOption::LightAccent) {
                    theme.light_highlight = defaults.light_highlight;
                } else if matches!(option, PreferenceOption::DarkAccent) {
                    theme.dark_highlight = defaults.dark_highlight;
                } else if matches!(option, PreferenceOption::BackgroundOpacity) {
                    theme.background_opacity = defaults.background_opacity;
                } else if matches!(option, PreferenceOption::ContextMenuBlurStrength) {
                    theme.context_menu_blur_strength = defaults.context_menu_blur_strength;
                } else if matches!(option, PreferenceOption::ContextMenuBlurKernelSize) {
                    theme.context_menu_blur_kernel_size = defaults.context_menu_blur_kernel_size;
                } else {
                    theme.border_radius = defaults.border_radius;
                }
                let Some(path) = self.active_profile.clone() else {
                    self.status = "No active configuration profile".into();
                    return;
                };
                let Some(profile) = self.profiles.iter().find(|profile| profile.path == path)
                else {
                    self.status = "The active configuration profile is unavailable".into();
                    return;
                };
                match self.config_store.save_theme_settings(profile, theme) {
                    Ok(profile) => self.apply_saved_profile(profile),
                    Err(error) => self.status = error,
                }
            }
            PreferenceOption::Layout
            | PreferenceOption::SmoothScrolling
            | PreferenceOption::ItemSize
            | PreferenceOption::MaxNameLines
            | PreferenceOption::Preview
            | PreferenceOption::SingleClickFolders
            | PreferenceOption::IconTheme
            | PreferenceOption::ThumbnailLocation
            | PreferenceOption::Terminal
            | PreferenceOption::FileContextMenuItems
            | PreferenceOption::FolderContextMenuItems
            | PreferenceOption::QuickToolbarItems
            | PreferenceOption::KeyboardShortcuts => {
                let mut browser = self.active_browser_settings();
                match option {
                    PreferenceOption::Layout => browser.layout = defaults.layout,
                    PreferenceOption::SmoothScrolling => {
                        browser.smooth_scrolling = defaults.smooth_scrolling
                    }
                    PreferenceOption::ItemSize => browser.item_size = defaults.item_size,
                    PreferenceOption::MaxNameLines => {
                        browser.max_name_lines = defaults.max_name_lines
                    }
                    PreferenceOption::Preview => browser.preview_enabled = defaults.preview_enabled,
                    PreferenceOption::SingleClickFolders => {
                        browser.single_click_opens_folders = defaults.single_click_opens_folders
                    }
                    PreferenceOption::IconTheme => browser.icon_theme = defaults.icon_theme,
                    PreferenceOption::ThumbnailLocation => {
                        browser.thumbnail_location = defaults.thumbnail_location
                    }
                    PreferenceOption::Terminal => {
                        browser.terminal_command = defaults.terminal_command
                    }
                    PreferenceOption::FileContextMenuItems => {
                        browser.file_context_menu_items = defaults.file_context_menu_items
                    }
                    PreferenceOption::FolderContextMenuItems => {
                        browser.folder_context_menu_items = defaults.folder_context_menu_items
                    }
                    PreferenceOption::QuickToolbarItems => {
                        browser.quick_toolbar_items = defaults.quick_toolbar_items
                    }
                    PreferenceOption::KeyboardShortcuts => {
                        browser.keyboard_shortcuts = defaults.keyboard_shortcuts
                    }
                    _ => unreachable!(),
                }
                self.save_browser_settings(browser);
            }
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

    fn save_background_opacity(&mut self, opacity: u8) {
        let Some(path) = self.active_profile.clone() else {
            self.status = "No active configuration profile".into();
            return;
        };
        let Some(profile) = self.profiles.iter().find(|profile| profile.path == path) else {
            self.status = "The active configuration profile is unavailable".into();
            return;
        };
        let mut theme = profile.theme.clone();
        theme.background_opacity = opacity;
        match self.config_store.save_theme_settings(profile, theme) {
            Ok(saved_profile) => self.apply_saved_profile(saved_profile),
            Err(error) => self.status = error,
        }
    }

    fn save_border_radius(&mut self, radius: u8) {
        let Some(path) = self.active_profile.clone() else {
            self.status = "No active configuration profile".into();
            return;
        };
        let Some(profile) = self.profiles.iter().find(|profile| profile.path == path) else {
            self.status = "The active configuration profile is unavailable".into();
            return;
        };
        let mut theme = profile.theme.clone();
        theme.border_radius = radius.min(8);
        match self.config_store.save_theme_settings(profile, theme) {
            Ok(saved_profile) => self.apply_saved_profile(saved_profile),
            Err(error) => self.status = error,
        }
    }

    fn save_context_menu_blur_strength(&mut self, strength: u8) {
        let Some(path) = self.active_profile.clone() else {
            self.status = "No active configuration profile".into();
            return;
        };
        let Some(profile) = self.profiles.iter().find(|profile| profile.path == path) else {
            self.status = "The active configuration profile is unavailable".into();
            return;
        };
        let mut theme = profile.theme.clone();
        theme.context_menu_blur_strength = strength.min(5);
        match self.config_store.save_theme_settings(profile, theme) {
            Ok(saved_profile) => self.apply_saved_profile(saved_profile),
            Err(error) => self.status = error,
        }
    }

    fn save_context_menu_blur_kernel_size(&mut self, kernel_size: ContextMenuBlurKernelSize) {
        let Some(path) = self.active_profile.clone() else {
            self.status = "No active configuration profile".into();
            return;
        };
        let Some(profile) = self.profiles.iter().find(|profile| profile.path == path) else {
            self.status = "The active configuration profile is unavailable".into();
            return;
        };
        let mut theme = profile.theme.clone();
        theme.context_menu_blur_kernel_size = kernel_size;
        match self.config_store.save_theme_settings(profile, theme) {
            Ok(saved_profile) => self.apply_saved_profile(saved_profile),
            Err(error) => self.status = error,
        }
    }

    fn save_context_menu_items(&mut self, is_directory: bool, items: Vec<ContextMenuItem>) {
        let Some(path) = self.active_profile.clone() else {
            self.status = "No active configuration profile".into();
            return;
        };
        let Some(profile) = self.profiles.iter().find(|profile| profile.path == path) else {
            self.status = "The active configuration profile is unavailable".into();
            return;
        };
        let mut browser = profile.browser.clone();
        if is_directory {
            browser.folder_context_menu_items = items;
        } else {
            browser.file_context_menu_items = items;
        }
        self.save_browser_settings(browser);
    }

    fn save_quick_toolbar_items(&mut self, items: Vec<QuickToolbarItem>) {
        let Some(path) = self.active_profile.clone() else {
            self.status = "No active configuration profile".into();
            return;
        };
        let Some(profile) = self.profiles.iter().find(|profile| profile.path == path) else {
            self.status = "The active configuration profile is unavailable".into();
            return;
        };
        let mut browser = profile.browser.clone();
        browser.quick_toolbar_items = items;
        self.save_browser_settings(browser);
    }

    fn save_folder_sort_override(&mut self, sort_order: Option<EntrySortOrder>) {
        let path = self.directory_path.clone();
        let mut browser = self.active_browser_settings();
        browser
            .folder_sort_overrides
            .retain(|override_| override_.path != path);
        if let Some(sort_order) = sort_order {
            browser
                .folder_sort_overrides
                .push(FolderSortOverride { path, sort_order });
        }
        sort_entries(&mut self.entries, sort_order.unwrap_or(browser.sort_order));
        self.save_browser_settings(browser);
    }

    fn save_keyboard_shortcut(&mut self, action: KeyboardShortcutAction, key: String) {
        let mut browser = self.active_browser_settings();
        browser
            .keyboard_shortcuts
            .retain(|shortcut| shortcut.action != action);
        let key = key.trim().to_owned();
        if !key.is_empty() {
            browser
                .keyboard_shortcuts
                .push(iron_file_common::config::KeyboardShortcut { action, key });
        }
        self.save_browser_settings(browser);
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
        set_border_radius(saved_profile.theme.border_radius);
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
}
