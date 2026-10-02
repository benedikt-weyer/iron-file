use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    Day,
    Night,
    System,
}

impl Default for ColorMode {
    fn default() -> Self {
        default_profile_file()
            .color_mode
            .expect("default profile must define color_mode")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub path: PathBuf,
    pub name: String,
    pub color_mode: ColorMode,
    pub sidebar_locations: Vec<SidebarLocation>,
    pub theme: ThemeSettings,
    pub browser: BrowserSettings,
    pub read_only: bool,
    pub base_profile: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidebarLocation {
    pub label: String,
    pub path: PathBuf,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeSettings {
    pub light_highlight: String,
    pub dark_highlight: String,
    #[serde(default = "default_background_opacity")]
    pub background_opacity: u8,
    #[serde(
        default = "default_context_menu_blur_strength",
        alias = "context_menu_blur"
    )]
    pub context_menu_blur_strength: u8,
    #[serde(default = "default_context_menu_blur_kernel_size")]
    pub context_menu_blur_kernel_size: ContextMenuBlurKernelSize,
    #[serde(default = "default_border_radius")]
    pub border_radius: u8,
}

pub(super) fn default_background_opacity() -> u8 {
    100
}

pub(super) fn default_context_menu_blur_strength() -> u8 {
    2
}

pub(super) fn default_context_menu_blur_kernel_size() -> ContextMenuBlurKernelSize {
    ContextMenuBlurKernelSize::Dynamic
}

pub(super) fn default_border_radius() -> u8 {
    6
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuBlurKernelSize {
    Fixed(u8),
    Dynamic,
}

impl ContextMenuBlurKernelSize {
    const FIXED_SIZES: [u8; 15] = [3, 5, 7, 9, 11, 13, 15, 17, 19, 21, 23, 25, 27, 29, 31];

    pub const OPTIONS: [Self; 16] = [
        Self::Fixed(3),
        Self::Fixed(5),
        Self::Fixed(7),
        Self::Fixed(9),
        Self::Fixed(11),
        Self::Fixed(13),
        Self::Fixed(15),
        Self::Fixed(17),
        Self::Fixed(19),
        Self::Fixed(21),
        Self::Fixed(23),
        Self::Fixed(25),
        Self::Fixed(27),
        Self::Fixed(29),
        Self::Fixed(31),
        Self::Dynamic,
    ];

    pub fn effective_size(self, strength: u8) -> u8 {
        match self {
            Self::Fixed(size) => size,
            Self::Dynamic => strength.saturating_mul(6).saturating_add(1).clamp(3, 31),
        }
    }

    pub(super) fn fixed(size: u8) -> Result<Self, String> {
        if Self::FIXED_SIZES.contains(&size) {
            Ok(Self::Fixed(size))
        } else {
            Err(format!("unsupported context menu blur kernel size: {size}"))
        }
    }
}

impl Default for ContextMenuBlurKernelSize {
    fn default() -> Self {
        default_context_menu_blur_kernel_size()
    }
}

impl fmt::Display for ContextMenuBlurKernelSize {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fixed(size) => write!(formatter, "{size} x {size}"),
            Self::Dynamic => formatter.write_str("6sigma + 1"),
        }
    }
}

impl Serialize for ContextMenuBlurKernelSize {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Fixed(size) => serializer.serialize_u8(*size),
            Self::Dynamic => serializer.serialize_str("6sigma+1"),
        }
    }
}

impl<'de> Deserialize<'de> for ContextMenuBlurKernelSize {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct KernelSizeVisitor;

        impl<'de> de::Visitor<'de> for KernelSizeVisitor {
            type Value = ContextMenuBlurKernelSize;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("an odd kernel size from 3 to 31 or 6sigma+1")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let size = u8::try_from(value).map_err(E::custom)?;
                ContextMenuBlurKernelSize::fixed(size).map_err(E::custom)
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let size = u8::try_from(value).map_err(E::custom)?;
                ContextMenuBlurKernelSize::fixed(size).map_err(E::custom)
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value == "6sigma+1" {
                    Ok(ContextMenuBlurKernelSize::Dynamic)
                } else {
                    value
                        .parse::<u8>()
                        .map_err(E::custom)
                        .and_then(|size| ContextMenuBlurKernelSize::fixed(size).map_err(E::custom))
                }
            }
        }

        deserializer.deserialize_any(KernelSizeVisitor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowserLayout {
    List,
    Tiles,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NameAlignment {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContextMenuItem {
    Info,
    CreateFolder,
    CreateFile,
    Rename,
    Duplicate,
    Open,
    OpenWith,
    CopyLocation,
    CopySelection,
    DeleteSelection,
    Paste,
    ToggleSidebarLocation,
    CreateSymlink,
    AddSymlinkToPasteBuffer,
    OpenTerminal,
}

impl ContextMenuItem {
    pub const ALL: [Self; 15] = [
        Self::Info,
        Self::CreateFolder,
        Self::CreateFile,
        Self::Rename,
        Self::Duplicate,
        Self::Open,
        Self::OpenWith,
        Self::CopyLocation,
        Self::CopySelection,
        Self::DeleteSelection,
        Self::Paste,
        Self::ToggleSidebarLocation,
        Self::CreateSymlink,
        Self::AddSymlinkToPasteBuffer,
        Self::OpenTerminal,
    ];

    pub const FILE_OPTIONS: [Self; 8] = [
        Self::Info,
        Self::Open,
        Self::OpenWith,
        Self::Rename,
        Self::Duplicate,
        Self::CopyLocation,
        Self::CopySelection,
        Self::DeleteSelection,
    ];

    pub const FOLDER_OPTIONS: [Self; 13] = [
        Self::Info,
        Self::CreateFolder,
        Self::CreateFile,
        Self::Rename,
        Self::Duplicate,
        Self::CopyLocation,
        Self::CopySelection,
        Self::DeleteSelection,
        Self::Paste,
        Self::ToggleSidebarLocation,
        Self::CreateSymlink,
        Self::AddSymlinkToPasteBuffer,
        Self::OpenTerminal,
    ];
}

impl fmt::Display for ContextMenuItem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Info => "Info",
            Self::CreateFolder => "Create folder",
            Self::CreateFile => "Create file",
            Self::Rename => "Rename",
            Self::Duplicate => "Duplicate",
            Self::Open => "Open",
            Self::OpenWith => "Open with...",
            Self::CopyLocation => "Copy location",
            Self::CopySelection => "Copy selection",
            Self::DeleteSelection => "Delete selection",
            Self::Paste => "Paste",
            Self::ToggleSidebarLocation => "Add or remove sidebar location",
            Self::CreateSymlink => "Create symlink",
            Self::AddSymlinkToPasteBuffer => "Add symlink to paste buffer",
            Self::OpenTerminal => "Open terminal",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuickToolbarItem {
    Refresh,
    CloneWindow,
    PerformanceDebugger,
    ToggleHiddenFiles,
    Sort,
    FolderSort,
    CompressSelection,
    ExtractSelection,
}

impl QuickToolbarItem {
    pub const ALL: [Self; 8] = [
        Self::Refresh,
        Self::CloneWindow,
        Self::PerformanceDebugger,
        Self::ToggleHiddenFiles,
        Self::Sort,
        Self::FolderSort,
        Self::CompressSelection,
        Self::ExtractSelection,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntrySortOrder {
    NameAscending,
    NameDescending,
    ModifiedNewest,
    ModifiedOldest,
    CreatedNewest,
    CreatedOldest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderSortOverride {
    pub path: PathBuf,
    pub sort_order: EntrySortOrder,
}

impl EntrySortOrder {
    pub const ALL: [Self; 6] = [
        Self::NameAscending,
        Self::NameDescending,
        Self::ModifiedNewest,
        Self::ModifiedOldest,
        Self::CreatedNewest,
        Self::CreatedOldest,
    ];
}

impl fmt::Display for EntrySortOrder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NameAscending => "Name (A-Z)",
            Self::NameDescending => "Name (Z-A)",
            Self::ModifiedNewest => "Last modified (newest)",
            Self::ModifiedOldest => "Last modified (oldest)",
            Self::CreatedNewest => "Created (newest)",
            Self::CreatedOldest => "Created (oldest)",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeyboardShortcutAction {
    RenameSelection,
    SearchCurrentFolder,
}

impl KeyboardShortcutAction {
    pub const ALL: [Self; 2] = [Self::RenameSelection, Self::SearchCurrentFolder];
}

impl fmt::Display for KeyboardShortcutAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RenameSelection => "Rename selected entry",
            Self::SearchCurrentFolder => "Search current folder",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardShortcut {
    pub action: KeyboardShortcutAction,
    pub key: String,
}

impl fmt::Display for QuickToolbarItem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Refresh => "Refresh folder",
            Self::CloneWindow => "Clone window",
            Self::PerformanceDebugger => "Performance debugger",
            Self::ToggleHiddenFiles => "Show hidden files",
            Self::Sort => "Sort entries",
            Self::FolderSort => "Override folder sorting",
            Self::CompressSelection => "Compress selection",
            Self::ExtractSelection => "Extract selection",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSettings {
    pub item_size: u16,
    pub layout: BrowserLayout,
    #[serde(default = "default_name_alignment")]
    pub name_alignment: NameAlignment,
    #[serde(default = "default_smooth_scrolling")]
    pub smooth_scrolling: bool,
    #[serde(default = "default_scroll_step")]
    pub scroll_step: u16,
    #[serde(default = "default_max_name_lines")]
    pub max_name_lines: u8,
    #[serde(default = "default_preview_enabled")]
    pub preview_enabled: bool,
    #[serde(default = "default_show_hidden_files")]
    pub show_hidden_files: bool,
    #[serde(default = "default_single_click_opens_folders")]
    pub single_click_opens_folders: bool,
    #[serde(default = "default_terminal_command")]
    pub terminal_command: String,
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: u16,
    #[serde(default = "default_icon_theme")]
    pub icon_theme: String,
    #[serde(default = "default_thumbnail_location")]
    pub thumbnail_location: PathBuf,
    #[serde(default = "default_file_context_menu_items")]
    pub file_context_menu_items: Vec<ContextMenuItem>,
    #[serde(default = "default_folder_context_menu_items")]
    pub folder_context_menu_items: Vec<ContextMenuItem>,
    #[serde(default = "default_quick_toolbar_items")]
    pub quick_toolbar_items: Vec<QuickToolbarItem>,
    #[serde(default = "default_entry_sort_order")]
    pub sort_order: EntrySortOrder,
    #[serde(default)]
    pub folder_sort_overrides: Vec<FolderSortOverride>,
    #[serde(default = "default_keyboard_shortcuts")]
    pub keyboard_shortcuts: Vec<KeyboardShortcut>,
    #[serde(default, rename = "context_menu_items", skip_serializing)]
    legacy_context_menu_items: Option<Vec<ContextMenuItem>>,
}

impl BrowserSettings {
    pub(super) fn apply_legacy_context_menu_items(&mut self) {
        let Some(items) = self.legacy_context_menu_items.take() else {
            return;
        };
        self.file_context_menu_items = items.clone();
        self.folder_context_menu_items = items;
    }
}
