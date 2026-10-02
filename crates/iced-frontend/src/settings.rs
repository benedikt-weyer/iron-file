use super::*;

impl Gui {
    pub(super) fn theme(&self) -> Theme {
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

    pub(super) fn active_theme_settings(&self) -> iron_file_common::config::ThemeSettings {
        self.active_profile
            .as_deref()
            .and_then(|path| self.profiles.iter().find(|profile| profile.path == path))
            .map(|profile| profile.theme.clone())
            .unwrap_or_else(iron_file_common::config::default_theme_settings)
    }

    pub(super) fn active_browser_settings(&self) -> BrowserSettings {
        self.active_profile
            .as_deref()
            .and_then(|path| self.profiles.iter().find(|profile| profile.path == path))
            .map(|profile| profile.browser.clone())
            .unwrap_or_else(iron_file_common::config::default_browser_settings)
    }

    pub(super) fn folder_sort_override(&self, path: &Path) -> Option<EntrySortOrder> {
        self.active_browser_settings()
            .folder_sort_overrides
            .into_iter()
            .find(|override_| override_.path == path)
            .map(|override_| override_.sort_order)
    }

    pub(super) fn current_sort_order(&self) -> EntrySortOrder {
        self.folder_sort_override(&self.directory_path)
            .unwrap_or_else(|| self.active_browser_settings().sort_order)
    }

    pub(super) fn sidebar_width(&self) -> u16 {
        self.sidebar_resize
            .map(|(_, _, width)| width)
            .unwrap_or_else(|| self.active_browser_settings().sidebar_width)
    }

    pub(super) fn terminal_choices(&self) -> Vec<String> {
        let mut choices = vec![DEFAULT_TERMINAL_CHOICE.into()];
        choices.extend(self.terminal_recommendations.clone());
        choices.push(CUSTOM_TERMINAL_CHOICE.into());
        choices
    }

    pub(super) fn selected_terminal_choice(&self, browser: &BrowserSettings) -> String {
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

    pub(super) fn icon_theme_choices(&self, browser: &BrowserSettings) -> Vec<String> {
        let mut themes = self.icon_themes.clone();
        if !themes.contains(&browser.icon_theme) {
            themes.push(browser.icon_theme.clone());
        }
        themes
    }

    pub(super) fn refresh_entry_icons(&mut self) {
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

    pub(super) fn cached_entry_icon_path(
        &mut self,
        theme: &str,
        entry: &proto::FileEntry,
    ) -> Option<PathBuf> {
        self.cached_entry_icon_path_with_names(theme, entry, None)
    }

    pub(super) fn cached_entry_icon_path_with_names(
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
}
