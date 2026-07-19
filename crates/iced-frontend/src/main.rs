use std::path::PathBuf;

use iced::{
    Element, Font, Length, Task, Theme,
    widget::{button, column, container, radio, row, scrollable, text, text_input, tooltip},
};
use iconflow::{Pack, Size, Style, fonts, try_icon};
use iron_file_common::{
    browse,
    config::{ColorMode, ConfigStore, Profile},
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
    ColorModeSelected(ColorMode),
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
            Message::ColorModeSelected(color_mode) => {
                self.save_color_mode(color_mode);
                Task::none()
            }
            Message::BrowseFinished(result) => {
                self.apply_response(result);
                Task::none()
            }
            Message::IconFontLoaded(_) => Task::none(),
        }
    }

    fn theme(&self) -> Theme {
        match self.color_mode {
            ColorMode::Day => Theme::Light,
            ColorMode::Night => Theme::Dark,
            ColorMode::System => Theme::default(),
        }
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
            Ok(saved_profile) => {
                let saved_path = saved_profile.path.clone();
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
            Err(error) => self.status = error,
        }
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
            column.push(
                button(row![icon, text(&entry.name)].spacing(8))
                    .width(Length::Fill)
                    .on_press(Message::OpenPath(PathBuf::from(&entry.path))),
            )
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

        container(
            column![address_bar, text(&self.status), browser]
                .spacing(12)
                .padding(16)
                .height(Length::Fill),
        )
        .width(Length::Fill)
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
                column![text("Color mode").size(18), options].spacing(10),
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
