mod backdrop_blur;
mod browser_view;
mod navigation;
mod preferences_view;
mod profile_settings;
mod sidebar;
mod startup;
mod types;
mod update_browser;
mod update_context;
mod update_loading;
mod update_preferences;
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
        self.update_browser(message)
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
