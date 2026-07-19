use std::{path::PathBuf, process::Command};

use iced::{
    Border, Color, Element, Font, Length, Point, Task, Theme, mouse,
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
use serde::Deserialize;
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
    drives: Vec<Drive>,
    content: String,
    status: String,
    editing_address: bool,
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

#[derive(Debug, Clone)]
struct Drive {
    path: PathBuf,
    name: String,
    mount_point: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Browser,
    Preferences,
}

#[derive(Debug, Clone)]
enum Message {
    AddressChanged(String),
    StartAddressEdit,
    CancelAddressEdit,
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
    DrivesLoaded(Result<Vec<Drive>, String>),
    MountDrive(PathBuf),
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
            drives: Vec::new(),
            content: String::new(),
            status: "Connecting to backend".into(),
            editing_address: false,
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
                )))
                .chain(std::iter::once(Task::perform(
                    load_drives(),
                    Message::DrivesLoaded,
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
            Message::DrivesLoaded(result) => {
                match result {
                    Ok(drives) => self.drives = drives,
                    Err(error) => self.status = error,
                }
                Task::none()
            }
            Message::MountDrive(path) => {
                self.status = format!("Mounting {}", path.display());
                Task::perform(mount_drive(path), Message::DrivesLoaded)
            }
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
        self.editing_address = false;
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
                    mouse_area(
                        button(row![icon, text(&entry.name)].spacing(8))
                            .style(iced::widget::button::text)
                            .width(Length::Fill)
                            .on_press(Message::OpenPath(path.clone())),
                    )
                    .on_right_press(Message::ShowFolderContext(path.clone()))
                    .interaction(mouse::Interaction::Pointer),
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

        let mut address_bar = row![
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
        let browser = row![
            scrollable(entries).width(Length::FillPortion(1)),
            scrollable(text(&self.content)).width(Length::FillPortion(2)),
        ]
        .spacing(16)
        .width(Length::Fill)
        .height(Length::Fill);
        let main_content = row![self.sidebar_view(), browser]
            .spacing(16)
            .width(Length::Fill)
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

        let page: Element<'_, Message> = if self.editing_address {
            stack![
                mouse_area(Space::new(Length::Fill, Length::Fill))
                    .on_press(Message::CancelAddressEdit)
                    .on_move(Message::ContextPointerMoved),
                page,
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            mouse_area(page)
                .on_move(Message::ContextPointerMoved)
                .into()
        };

        stack![page]
            .push_maybe(overlay)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
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
        let drives = self.drives.iter().fold(
            column![text("Drives").size(16)].spacing(6),
            |column, drive| {
                let label = if let Some(mount_point) = &drive.mount_point {
                    format!("{} ({})", drive.name, mount_point.display())
                } else {
                    drive.name.clone()
                };
                let item = row![icon_text("hard-drive"), text(label)].spacing(8);
                if let Some(mount_point) = &drive.mount_point {
                    column.push(
                        button(item)
                            .style(iced::widget::button::text)
                            .width(Length::Fill)
                            .on_press(Message::OpenPath(mount_point.clone())),
                    )
                } else {
                    column.push(
                        row![
                            container(item).width(Length::Fill),
                            tooltip(
                                button(icon_text("plug-zap"))
                                    .on_press(Message::MountDrive(drive.path.clone())),
                                text("Mount drive"),
                                tooltip::Position::Right,
                            ),
                        ]
                        .spacing(4),
                    )
                }
            },
        );
        let sidebar_content = column![locations, drives].spacing(20);
        container(scrollable(sidebar_content))
            .width(Length::Fixed(180.0))
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

fn icon_text<'a>(name: &str) -> iced::widget::Text<'a> {
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

async fn load_drives() -> Result<Vec<Drive>, String> {
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
        Ok(drives)
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(Vec::new())
    }
}

#[cfg(target_os = "linux")]
fn collect_drives(device: LsblkDevice, drives: &mut Vec<Drive>) {
    let is_volume = matches!(device.device_type.as_str(), "disk" | "part")
        && (device.device_type == "part" || device.children.is_empty());
    if is_volume {
        if let Some(path) = device.path {
            let mount_point = device.mountpoints.into_iter().flatten().next();
            drives.push(Drive {
                path,
                name: device.label.unwrap_or(device.name),
                mount_point,
            });
        }
    }
    for child in device.children {
        collect_drives(child, drives);
    }
}

async fn mount_drive(path: PathBuf) -> Result<Vec<Drive>, String> {
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
        load_drives().await
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err("Mounting drives is not supported on this platform".into())
    }
}
