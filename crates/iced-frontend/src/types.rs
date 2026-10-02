use super::*;

pub(super) struct Gui {
    pub(super) follow_logs: bool,
    pub(super) picker: Option<PickerOptions>,
    pub(super) save_file_name: Option<String>,
    pub(super) original_save_file_name: Option<String>,
    pub(super) directory_path: PathBuf,
    pub(super) address: String,
    pub(super) entries: Vec<proto::FileEntry>,
    pub(super) drives: Vec<Drive>,
    pub(super) mounts: Vec<SystemMount>,
    pub(super) content: String,
    pub(super) status: String,
    pub(super) editing_address: bool,
    pub(super) view: View,
    pub(super) config_store: ConfigStore,
    pub(super) profiles: Vec<Profile>,
    pub(super) active_profile: Option<PathBuf>,
    pub(super) new_profile_name: String,
    pub(super) color_mode: ColorMode,
    pub(super) light_accent_input: String,
    pub(super) dark_accent_input: String,
    pub(super) accent_picker: Option<AccentPickerState>,
    pub(super) context_entry: Option<ContextEntry>,
    pub(super) show_performance_debugger: bool,
    pub(super) folder_load_started: Option<Instant>,
    pub(super) last_folder_load_duration: Option<Duration>,
    pub(super) folder_load_performance: Option<FolderLoadPerformance>,
    pub(super) pending_info: Option<InfoDialog>,
    pub(super) pointer_position: Point,
    pub(super) context_position: Point,
    pub(super) dragging_sidebar_location: Option<PathBuf>,
    pub(super) sidebar_drop_target: Option<PathBuf>,
    pub(super) sidebar_drop_at_end: bool,
    pub(super) dragging_entries: Option<Vec<PathBuf>>,
    pub(super) pending_drag: Option<(Point, Vec<PathBuf>)>,
    pub(super) entry_drop_target: Option<PathBuf>,
    pub(super) hovered_entry: Option<(PathBuf, bool)>,
    pub(super) window_cursor_position: Point,
    pub(super) last_entry_click: Option<(PathBuf, Instant)>,
    pub(super) terminal_recommendations: Vec<String>,
    pub(super) history: Vec<PathBuf>,
    pub(super) history_index: Option<usize>,
    pub(super) sidebar_resize: Option<(f32, u16, u16)>,
    pub(super) icon_themes: Vec<String>,
    pub(super) entry_icons: HashMap<PathBuf, Option<PathBuf>>,
    pub(super) entry_icon_cache: HashMap<(String, bool, Option<String>), Option<PathBuf>>,
    pub(super) thumbnail_handles: HashMap<PathBuf, image::Handle>,
    pub(super) pending_thumbnail_paths: Vec<PathBuf>,
    pub(super) selected_entries: HashSet<PathBuf>,
    pub(super) paste_buffer: Option<PasteBuffer>,
    pub(super) pending_delete: Option<Vec<PathBuf>>,
    pub(super) delete_confirm_selected: bool,
    pub(super) pending_create: Option<(PathBuf, bool)>,
    pub(super) create_entry_name: String,
    pub(super) pending_rename: Option<PathBuf>,
    pub(super) rename_entry_name: String,
    pub(super) pending_profile_reset: bool,
    pub(super) pending_compression: bool,
    pub(super) compression_level: u8,
    pub(super) compression_type: ArchiveCompression,
    pub(super) selection_anchor: Option<PathBuf>,
    pub(super) modifiers: keyboard::Modifiers,
    pub(super) browser_pointer: Point,
    pub(super) browser_scroll_offset: scrollable::AbsoluteOffset,
    pub(super) rectangle_selection: Option<RectangleSelection>,
    pub(super) search: Option<SearchState>,
    pub(super) tile_columns: Rc<Cell<usize>>,
}

pub(super) const DEFAULT_TERMINAL_CHOICE: &str = "System default";
pub(super) const CUSTOM_TERMINAL_CHOICE: &str = "Custom command";
pub(super) const RECOMMENDED_TERMINALS: &[&str] = &[
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
pub(super) struct Drive {
    pub(super) path: PathBuf,
    pub(super) name: String,
    pub(super) mount_points: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub(super) struct SystemMount {
    pub(super) path: PathBuf,
    pub(super) filesystem: String,
}

#[derive(Debug, Clone)]
pub(super) struct MountState {
    pub(super) drives: Vec<Drive>,
    pub(super) mounts: Vec<SystemMount>,
}

#[derive(Debug, Clone)]
pub(super) struct ContextEntry {
    pub(super) path: PathBuf,
    pub(super) is_directory: bool,
    pub(super) is_sidebar_location: bool,
    pub(super) opener: Option<Result<String, String>>,
    pub(super) open_with_expanded: bool,
    pub(super) open_with_apps: Option<Result<Vec<AppChoice>, String>>,
}

#[derive(Debug, Clone)]
pub(super) struct FolderLoadPerformance {
    pub(super) started_at: Instant,
    pub(super) enumeration_ms: Option<u64>,
    pub(super) expected_entries: usize,
    pub(super) first_item_at: Option<Instant>,
    pub(super) displayed_at: Option<Instant>,
    pub(super) thumbnails_total: usize,
    pub(super) thumbnails_settled: usize,
    pub(super) thumbnails_settled_at: Option<Instant>,
    pub(super) item_rendered_at: Vec<Instant>,
    pub(super) thumbnail_settled_at: Vec<Instant>,
    pub(super) entry_path_us: u64,
    pub(super) entry_symlink_us: u64,
    pub(super) entry_metadata_us: u64,
    pub(super) entry_timestamps_us: u64,
}

#[derive(Debug, Clone)]
pub(super) enum InfoDialog {
    Loading(PathBuf),
    Loaded(EntryInfo),
    Error { path: PathBuf, error: String },
}

#[derive(Debug, Clone)]
pub(super) struct EntryInfo {
    pub(super) path: PathBuf,
    pub(super) name: String,
    pub(super) rows: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub(super) struct RectangleSelection {
    pub(super) start: Point,
    pub(super) end: Point,
    pub(super) initial_selection: HashSet<PathBuf>,
}

pub(super) const PROGRESSIVE_SEARCH_MAX_DEPTH: u32 = 64;

#[derive(Debug, Clone)]
pub(super) struct SearchState {
    pub(super) root: PathBuf,
    pub(super) query: String,
    pub(super) depth: SearchDepth,
    pub(super) in_progress: bool,
    /// The max-depth of the request currently in flight, and (for a
    /// progressive search) the number of entries its previous step found,
    /// used to decide whether to keep going deeper.
    pub(super) pending_request: Option<(u32, Option<usize>)>,
    pub(super) cancel_handle: Option<task::Handle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SearchDepth {
    CurrentFolder,
    OneLevel,
    TwoLevels,
    ThreeLevels,
    Maximum,
    Progressive,
}

impl SearchDepth {
    pub(super) const ALL: [Self; 6] = [
        Self::CurrentFolder,
        Self::OneLevel,
        Self::TwoLevels,
        Self::ThreeLevels,
        Self::Maximum,
        Self::Progressive,
    ];

    pub(super) fn max_depth(self) -> u32 {
        match self {
            Self::CurrentFolder => 0,
            Self::OneLevel => 1,
            Self::TwoLevels => 2,
            Self::ThreeLevels => 3,
            Self::Maximum => u32::MAX,
            Self::Progressive => 0,
        }
    }
}

impl std::fmt::Display for SearchDepth {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::CurrentFolder => "Depth: 0",
            Self::OneLevel => "Depth: 1",
            Self::TwoLevels => "Depth: 2",
            Self::ThreeLevels => "Depth: 3",
            Self::Maximum => "Depth: Max",
            Self::Progressive => "Progressive",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum View {
    Browser,
    Preferences,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum HistoryRequest {
    Initial,
    New,
    Existing(usize),
    Refresh,
}

#[derive(Debug, Clone)]
pub(super) enum BrowserCommand {
    CopySelection,
    CutSelection,
    CancelCut,
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
pub(super) enum SelectionDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FolderSortSelection {
    None,
    NameAscending,
    NameDescending,
    ModifiedNewest,
    ModifiedOldest,
    CreatedNewest,
    CreatedOldest,
}

impl FolderSortSelection {
    pub(super) const ALL: [Self; 7] = [
        Self::None,
        Self::NameAscending,
        Self::NameDescending,
        Self::ModifiedNewest,
        Self::ModifiedOldest,
        Self::CreatedNewest,
        Self::CreatedOldest,
    ];

    pub(super) fn sort_order(self) -> Option<EntrySortOrder> {
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
pub(super) enum PasteMode {
    Copy,
    Move,
    Symlink,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct AccentPickerState {
    pub(super) dark: bool,
    pub(super) hue: u16,
    pub(super) saturation: u8,
    pub(super) value: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ArchiveCompression {
    Store,
    Deflate,
    Bzip2,
    Zstd,
}

impl ArchiveCompression {
    pub(super) fn value(self) -> &'static str {
        match self {
            Self::Store => "store",
            Self::Deflate => "deflate",
            Self::Bzip2 => "bzip2",
            Self::Zstd => "zstd",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum PreferenceOption {
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
    NameAlignment,
    SmoothScrolling,
    ScrollStep,
    ItemSize,
    MaxNameLines,
    Preview,
    SingleClickFolders,
    IconTheme,
    ThumbnailLocation,
    Terminal,
}

#[derive(Debug, Clone)]
pub(super) struct PasteBuffer {
    pub(super) entries: Vec<PathBuf>,
    pub(super) mode: PasteMode,
}

#[derive(Debug, Clone)]
pub(super) enum Message {
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
    EntryHovered {
        path: PathBuf,
        is_directory: bool,
    },
    EntryUnhovered(PathBuf),
    LeftMouseButtonPressed,
    LeftMouseButtonReleased,
    WindowCursorMoved(Point),
    ExecuteBrowserCommand(BrowserCommand),
    FileCopyFinished(Result<Vec<PathBuf>, String>),
    CutPasteFinished {
        sources: Vec<PathBuf>,
        result: Result<Vec<PathBuf>, String>,
    },
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
    BrowserScrolled(scrollable::Viewport),
    OpenParent,
    ShowSearch,
    CloseSearch,
    SearchQueryChanged(String),
    SearchDepthSelected(SearchDepth),
    RunSearch,
    CancelSearch,
    SearchFinished {
        root: PathBuf,
        query: String,
        depth: SearchDepth,
        requested_depth: u32,
        result: Result<Vec<proto::FileEntry>, String>,
    },
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
    NameAlignmentSelected(NameAlignment),
    SmoothScrollingToggled(bool),
    ScrollStepChanged(u16),
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
    ShowSidebarLocationContext(PathBuf),
    SetSidebarLocationIcon {
        path: PathBuf,
        icon: Option<String>,
    },
    FileOpenerResolved {
        path: PathBuf,
        opener: Result<String, String>,
    },
    ContextPointerMoved(Point),
    CloseFolderContext,
    TogglePerformanceDebugger,
    CopyPerformanceReport,
    RequestEntryInfo(PathBuf),
    EntryInfoLoaded {
        path: PathBuf,
        result: Result<proto::EntryInfoResponse, String>,
    },
    CloseEntryInfo,
    OpenContextFile,
    ToggleOpenWith,
    OpenWithAppsLoaded {
        path: PathBuf,
        apps: Result<Vec<AppChoice>, String>,
    },
    OpenWithApp(String),
    SetDefaultApp(String),
    DefaultAppSet {
        path: PathBuf,
        result: Result<(), String>,
    },
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
    RefreshDirectory,
    CloneWindow,
    DuplicateContextEntry(PathBuf),
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
    DirectoryEntriesLoaded {
        directory: PathBuf,
        entries: Result<Vec<proto::FileEntry>, String>,
    },
    BrowseFinished {
        result: Result<BrowseResponse, String>,
        history: HistoryRequest,
    },
    IconFontLoaded(Result<(), iced::font::Error>),
}
