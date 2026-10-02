use super::*;

impl Gui {
    pub(super) fn save_browser_settings(&mut self, browser: BrowserSettings) {
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

    pub(super) fn select_profile(&mut self, path: PathBuf) {
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

    pub(super) fn create_profile(&mut self) {
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

    pub(super) fn save_color_mode(&mut self, color_mode: ColorMode) {
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

    pub(super) fn reset_preference(&mut self, option: PreferenceOption) {
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
            | PreferenceOption::NameAlignment
            | PreferenceOption::SmoothScrolling
            | PreferenceOption::ScrollStep
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
                    PreferenceOption::NameAlignment => {
                        browser.name_alignment = defaults.name_alignment
                    }
                    PreferenceOption::SmoothScrolling => {
                        browser.smooth_scrolling = defaults.smooth_scrolling
                    }
                    PreferenceOption::ScrollStep => browser.scroll_step = defaults.scroll_step,
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

    pub(super) fn save_accent_color(&mut self, dark: bool, value: String) {
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

    pub(super) fn save_background_opacity(&mut self, opacity: u8) {
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

    pub(super) fn save_border_radius(&mut self, radius: u8) {
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

    pub(super) fn save_context_menu_blur_strength(&mut self, strength: u8) {
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

    pub(super) fn save_context_menu_blur_kernel_size(
        &mut self,
        kernel_size: ContextMenuBlurKernelSize,
    ) {
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

    pub(super) fn save_context_menu_items(
        &mut self,
        is_directory: bool,
        items: Vec<ContextMenuItem>,
    ) {
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

    pub(super) fn save_quick_toolbar_items(&mut self, items: Vec<QuickToolbarItem>) {
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

    pub(super) fn save_folder_sort_override(&mut self, sort_order: Option<EntrySortOrder>) {
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

    pub(super) fn save_keyboard_shortcut(&mut self, action: KeyboardShortcutAction, key: String) {
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

    pub(super) fn save_sidebar_locations(&mut self, sidebar_locations: Vec<SidebarLocation>) {
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

    pub(super) fn apply_saved_profile(&mut self, saved_profile: Profile) {
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

    pub(super) fn reset_active_profile(&mut self) {
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
}
