use std::path::PathBuf;

use iced::{
    Color, Element, Font, Length, Point, Task, Theme,
    widget::{
        Space, button, column, container, mouse_area, radio, row, scrollable, stack, text,
        text_input, tooltip,
    },
};
use iconflow::{Pack, Size, Style, fonts, try_icon};
use iron_file_common::{
    browse,
    config::{ColorMode, ConfigStore, Profile, SidebarLocation},
    ensure_backend, proto,
};
use proto::{BrowseResponse, browse_response::Payload};
use tokio::runtime::Runtime;

fn main() -> iced::Result {
    prefer_x11_when_available();
    if let Ok(runtime) = Runtime::new() {
        let _ = runtime.block_on(ensure_backend());
    }
    iced::application("Iron File", Gui::update, Gui::view)
        .theme(Gui::theme)
        .run_with(|| (Gui::new(), Gui::load_initial_directory()))
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
    content: String,
    status: String,
    view: View,
    config_store: ConfigStore,
    profiles: Vec<Profile>,
    active_profile: Option<PathBuf>,
    new_profile_name: String,
    color_mode: ColorMode,
    context_folder: Option<PathBuf>,
    pointer_position: Point,
    context_position: Point,
    dragging_sidebar_location: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Browser,
    Preferences,
}

#[derive(Debug, Clone)]
enum Message {
    AddressChanged(String),
    OpenAddress,
    OpenPath(PathBuf),
    OpenParent,
    ShowBrowser,
    ShowPreferences,
    SelectProfile(PathBuf),
    NewProfileNameChanged(String),
    CreateProfile,
    ResetActiveProfile,
    ColorModeSelected(ColorMode),
    ShowFolderContext(PathBuf),
    ContextPointerMoved(Point),
    CloseFolderContext,
    AddContextFolderToSidebar,
    RemoveContextFolderFromSidebar,
    SidebarPressed(PathBuf),
    SidebarReleased(PathBuf),
    BrowseFinished(Result<BrowseResponse, String>),
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
            content: String::new(),
            status: "Connecting to backend".into(),
            view: View::Browser,
            config_store,
            profiles,
            active_profile,
            new_profile_name: String::new(),
            color_mode,
            context_folder: None,
            pointer_position: Point::ORIGIN,
            context_position: Point::ORIGIN,
            dragging_sidebar_location: None,
        }
    }

    fn load_initial_directory() -> Task<Message> {
        let path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Task::batch(
            fonts()
                .iter()
                .map(|font| iced::font::load(font.bytes).map(Message::IconFontLoaded))
                .chain(std::iter::once(Task::perform(
                    browse(path),
                    Message::BrowseFinished,
                ))),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::AddressChanged(address) => {
                self.address = address;
                Task::none()
            }
            Message::OpenAddress => self.open_path(PathBuf::from(&self.address)),
            Message::OpenPath(path) => self.open_path(path),
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
            Message::ShowFolderContext(path) => {
                self.context_folder = Some(path);
                self.context_position = self.pointer_position;
                Task::none()
            }
            Message::ContextPointerMoved(position) => {
                self.pointer_position = position;
                Task::none()
            }
            Message::CloseFolderContext => {
                self.context_folder = None;
                Task::none()
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
            Message::BrowseFinished(result) => {
                self.apply_response(result);
                Task::none()
            }
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

    fn select_profile(&mut self, path: PathBuf) {
        let Some(profile) = self.profiles.iter().find(|profile| profile.path == path) else {
            return;
        };
        self.active_profile = Some(path.clone());
        self.color_mode = profile.color_mode;
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
        let Some(path) = self.context_folder.take() else {
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
        let Some(path) = self.context_folder.take() else {
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
        self.status = format!("Loading {}", path.display());
        Task::perform(browse(path), Message::BrowseFinished)
    }

    fn apply_response(&mut self, result: Result<BrowseResponse, String>) {
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                self.status = error;
                return;
            }
        };

        match response.payload {
            Some(Payload::Directory(directory)) => {
                self.address = response.path.clone();
                self.directory_path = PathBuf::from(response.path);
                self.entries = directory.entries;
                self.content.clear();
                self.status = format!("{} entries", self.entries.len());
            }
            Some(Payload::File(file)) => {
                self.address = response.path;
                self.content = file.content;
                self.status = "File preview".into();
            }
            Some(Payload::Error(error)) => self.status = error.message,
            None => self.status = "Backend returned an invalid response".into(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        match self.view {
            View::Browser => self.browser_view(),
            View::Preferences => self.preferences_view(),
        }
    }

    fn browser_view(&self) -> Element<'_, Message> {
        let entries = self.entries.iter().fold(column![], |column, entry| {
            let icon = if entry.is_directory {
                icon_text("folder")
            } else {
                icon_text("file")
            };
            let path = PathBuf::from(&entry.path);
            if entry.is_directory {
                column.push(
                    mouse_area(row![icon, text(&entry.name)].spacing(8))
                        .on_press(Message::OpenPath(path.clone()))
                        .on_right_press(Message::ShowFolderContext(path.clone())),
                )
            } else {
                column.push(
                    button(row![icon, text(&entry.name)].spacing(8))
                        .style(iced::widget::button::text)
                        .width(Length::Fill)
                        .on_press(Message::OpenPath(path)),
                )
            }
        });

        let address_bar = row![
            tooltip(
                button(icon_text("folder-up")).on_press(Message::OpenParent),
                text("Parent folder"),
                tooltip::Position::Bottom,
            ),
            text_input("Path", &self.address)
                .on_input(Message::AddressChanged)
                .on_submit(Message::OpenAddress)
                .width(Length::Fill),
            tooltip(
                button(icon_text("folder-open")).on_press(Message::OpenAddress),
                text("Open path"),
                tooltip::Position::Bottom,
            ),
            tooltip(
                button(icon_text("settings")).on_press(Message::ShowPreferences),
                text("Preferences"),
                tooltip::Position::Bottom,
            ),
        ]
        .spacing(8);
        let browser = row![
            scrollable(entries).width(Length::FillPortion(1)),
            scrollable(text(&self.content)).width(Length::FillPortion(2)),
        ]
        .spacing(16)
        .height(Length::Fill);
        let main_content = row![self.sidebar_view(), browser]
            .spacing(16)
            .height(Length::Fill);
        let content = column![address_bar, text(&self.status), main_content];

        let page = container(content.spacing(12).padding(16).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill);
        let overlay = self.context_folder.as_ref().map(|folder| {
            let is_in_sidebar = self
                .active_sidebar_locations()
                .iter()
                .any(|location| location.path == *folder);
            let action = if is_in_sidebar {
                button(text("Remove from sidebar"))
                    .on_press(Message::RemoveContextFolderFromSidebar)
            } else {
                button(text("Add to sidebar")).on_press(Message::AddContextFolderToSidebar)
            };
            let menu = container(
                row![
                    action,
                    tooltip(
                        button(icon_text("x")).on_press(Message::CloseFolderContext),
                        text("Close menu"),
                        tooltip::Position::Bottom,
                    ),
                ]
                .spacing(8),
            )
            .padding(8);
            let menu_position = container(column![
                Space::with_height(self.context_position.y),
                row![Space::with_width(self.context_position.x), menu],
            ])
            .width(Length::Fill)
            .height(Length::Fill);
            stack![
                mouse_area(Space::new(Length::Fill, Length::Fill))
                    .on_press(Message::CloseFolderContext),
                menu_position,
            ]
            .width(Length::Fill)
            .height(Length::Fill)
        });

        stack![mouse_area(page).on_move(Message::ContextPointerMoved)]
            .push_maybe(overlay)
            .width(Length::Fill)
            .height(Length::Fill)
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
        container(scrollable(locations))
            .width(180)
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

fn icon_text(name: &str) -> iced::widget::Text<'static> {
    let icon = try_icon(Pack::Lucide, name, Style::Regular, Size::Regular)
        .expect("missing bundled Lucide icon");
    let glyph = char::from_u32(icon.codepoint).unwrap_or('?');

    text(glyph.to_string())
        .size(18)
        .font(Font::with_name(icon.family))
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
