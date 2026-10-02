use super::*;

pub fn default_sidebar_locations() -> Vec<SidebarLocation> {
    default_profile_file()
        .sidebar_locations
        .expect("default profile must define sidebar_locations")
        .into_iter()
        .map(|mut location| {
            location.path = expand_home_path(&location.path);
            location
        })
        .collect()
}

pub fn default_theme_settings() -> ThemeSettings {
    default_profile_file()
        .theme
        .expect("default profile must define theme")
}

pub fn default_browser_settings() -> BrowserSettings {
    default_profile_file()
        .browser
        .expect("default profile must define browser")
}

pub(super) fn default_single_click_opens_folders() -> bool {
    default_browser_settings().single_click_opens_folders
}

pub(super) fn default_preview_enabled() -> bool {
    default_browser_settings().preview_enabled
}

pub(super) fn default_max_name_lines() -> u8 {
    default_browser_settings().max_name_lines
}

pub(super) fn default_name_alignment() -> NameAlignment {
    default_browser_settings().name_alignment
}

pub(super) fn default_smooth_scrolling() -> bool {
    default_browser_settings().smooth_scrolling
}

pub(super) fn default_scroll_step() -> u16 {
    default_browser_settings().scroll_step
}

pub(super) fn default_show_hidden_files() -> bool {
    default_browser_settings().show_hidden_files
}

pub(super) fn default_terminal_command() -> String {
    default_browser_settings().terminal_command
}

pub(super) fn default_sidebar_width() -> u16 {
    default_browser_settings().sidebar_width
}

pub(super) fn default_icon_theme() -> String {
    default_browser_settings().icon_theme
}

pub(super) fn default_thumbnail_location() -> PathBuf {
    default_browser_settings().thumbnail_location
}

pub(super) fn default_file_context_menu_items() -> Vec<ContextMenuItem> {
    ContextMenuItem::FILE_OPTIONS.to_vec()
}

pub(super) fn default_folder_context_menu_items() -> Vec<ContextMenuItem> {
    ContextMenuItem::FOLDER_OPTIONS.to_vec()
}

pub(super) fn default_quick_toolbar_items() -> Vec<QuickToolbarItem> {
    QuickToolbarItem::ALL.to_vec()
}

pub(super) fn default_entry_sort_order() -> EntrySortOrder {
    EntrySortOrder::NameAscending
}

pub(super) fn default_keyboard_shortcuts() -> Vec<KeyboardShortcut> {
    vec![
        KeyboardShortcut {
            action: KeyboardShortcutAction::RenameSelection,
            key: "F2".into(),
        },
        KeyboardShortcut {
            action: KeyboardShortcutAction::SearchCurrentFolder,
            key: "Ctrl+F".into(),
        },
    ]
}
