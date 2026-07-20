use std::{
    collections::HashSet,
    env, fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const APP_NAME: &str = "iron-file";
const PROFILES_DIRECTORY: &str = "profiles";
const STATE_FILE: &str = "config.toml";
const DEFAULT_PROFILE_TOML: &str = include_str!("../../../config/default.toml");

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeSettings {
    pub light_highlight: String,
    pub dark_highlight: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowserLayout {
    List,
    Tiles,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSettings {
    pub item_size: u16,
    pub layout: BrowserLayout,
    #[serde(default = "default_preview_enabled")]
    pub preview_enabled: bool,
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
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    user_config_dir: PathBuf,
    search_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct ProfileFile {
    name: Option<String>,
    color_mode: Option<ColorMode>,
    sidebar_locations: Option<Vec<SidebarLocation>>,
    theme: Option<ThemeSettings>,
    browser: Option<BrowserSettings>,
    base_profile: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct StateFile {
    active_profile: Option<PathBuf>,
}

impl ConfigStore {
    pub fn from_environment() -> Self {
        let user_config_dir = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join(APP_NAME);
        let system_dirs = env::var_os("XDG_CONFIG_DIRS")
            .map(|paths| env::split_paths(&paths).collect())
            .unwrap_or_else(|| vec![PathBuf::from("/etc/xdg")]);
        Self::with_paths(
            user_config_dir,
            system_dirs
                .into_iter()
                .map(|path| path.join(APP_NAME))
                .collect(),
        )
    }

    pub fn with_paths(user_config_dir: PathBuf, system_config_dirs: Vec<PathBuf>) -> Self {
        let mut search_paths = vec![user_config_dir.join(PROFILES_DIRECTORY)];
        search_paths.extend(
            system_config_dirs
                .into_iter()
                .map(|path| path.join(PROFILES_DIRECTORY)),
        );
        Self {
            user_config_dir,
            search_paths,
        }
    }

    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }

    pub fn profiles(&self) -> Result<Vec<Profile>, String> {
        let mut paths = Vec::new();
        let mut seen = HashSet::new();
        for directory in &self.search_paths {
            let entries = match fs::read_dir(directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(format!("Could not read {}: {error}", directory.display()));
                }
            };
            for entry in entries {
                let path = entry
                    .map_err(|error| format!("Could not read a profile entry: {error}"))?
                    .path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "toml")
                    && seen.insert(path.clone())
                {
                    paths.push(path);
                }
            }
        }
        let mut profiles = paths
            .iter()
            .map(|path| self.read_profile(path))
            .collect::<Result<Vec<_>, _>>()?;
        profiles.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        Ok(profiles)
    }

    pub fn create_profile(&self, name: &str) -> Result<Profile, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Profile name cannot be empty".into());
        }
        let filename = profile_filename(name)?;
        let path = self.user_profiles_dir().join(filename);
        if path.exists() {
            return Err(format!("A profile named {name} already exists"));
        }
        let file = ProfileFile {
            name: Some(name.into()),
            color_mode: Some(ColorMode::System),
            sidebar_locations: Some(default_sidebar_locations()),
            theme: Some(default_theme_settings()),
            browser: Some(default_browser_settings()),
            base_profile: None,
        };
        self.write_profile_file(&path, &file)?;
        self.read_profile(&path)
    }

    pub fn active_profile(&self) -> Result<Option<PathBuf>, String> {
        let path = self.user_config_dir.join(STATE_FILE);
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("Could not read {}: {error}", path.display())),
        };
        toml::from_str::<StateFile>(&content)
            .map(|state| state.active_profile.filter(|profile| profile.exists()))
            .map_err(|error| format!("Could not parse {}: {error}", path.display()))
    }

    pub fn set_active_profile(&self, profile: &Path) -> Result<(), String> {
        self.write_state(&StateFile {
            active_profile: Some(profile.to_path_buf()),
        })
    }

    pub fn save_color_mode(
        &self,
        profile: &Profile,
        color_mode: ColorMode,
    ) -> Result<Profile, String> {
        if profile.read_only {
            let overlay = self.overlay_path(&profile.path);
            let file = ProfileFile {
                name: Some(profile.name.clone()),
                color_mode: Some(color_mode),
                sidebar_locations: None,
                theme: None,
                browser: None,
                base_profile: Some(profile.path.clone()),
            };
            self.write_profile_file(&overlay, &file)?;
            return self.read_profile(&overlay);
        }

        let mut file = self.read_profile_file(&profile.path)?;
        file.color_mode = Some(color_mode);
        self.write_profile_file(&profile.path, &file)?;
        self.read_profile(&profile.path)
    }

    pub fn save_sidebar_locations(
        &self,
        profile: &Profile,
        sidebar_locations: Vec<SidebarLocation>,
    ) -> Result<Profile, String> {
        if profile.read_only {
            let overlay = self.overlay_path(&profile.path);
            let file = ProfileFile {
                name: Some(profile.name.clone()),
                color_mode: None,
                sidebar_locations: Some(sidebar_locations),
                theme: None,
                browser: None,
                base_profile: Some(profile.path.clone()),
            };
            self.write_profile_file(&overlay, &file)?;
            return self.read_profile(&overlay);
        }

        let mut file = self.read_profile_file(&profile.path)?;
        file.sidebar_locations = Some(sidebar_locations);
        self.write_profile_file(&profile.path, &file)?;
        self.read_profile(&profile.path)
    }

    pub fn save_browser_settings(
        &self,
        profile: &Profile,
        browser: BrowserSettings,
    ) -> Result<Profile, String> {
        if profile.read_only {
            let overlay = self.overlay_path(&profile.path);
            let file = ProfileFile {
                name: Some(profile.name.clone()),
                color_mode: None,
                sidebar_locations: None,
                theme: None,
                browser: Some(browser),
                base_profile: Some(profile.path.clone()),
            };
            self.write_profile_file(&overlay, &file)?;
            return self.read_profile(&overlay);
        }

        let mut file = self.read_profile_file(&profile.path)?;
        file.browser = Some(browser);
        self.write_profile_file(&profile.path, &file)?;
        self.read_profile(&profile.path)
    }

    pub fn reset_profile(&self, profile: &Profile) -> Result<Profile, String> {
        let defaults = default_profile_file();
        let color_mode = defaults
            .color_mode
            .expect("default profile must define color_mode");
        let sidebar_locations = default_sidebar_locations();
        let path = if profile.read_only {
            let overlay = self.overlay_path(&profile.path);
            let file = ProfileFile {
                name: Some(profile.name.clone()),
                color_mode: Some(color_mode),
                sidebar_locations: Some(sidebar_locations),
                theme: Some(default_theme_settings()),
                browser: Some(default_browser_settings()),
                base_profile: Some(profile.path.clone()),
            };
            self.write_profile_file(&overlay, &file)?;
            overlay
        } else {
            let mut file = self.read_profile_file(&profile.path)?;
            file.color_mode = Some(color_mode);
            file.sidebar_locations = Some(sidebar_locations);
            file.theme = Some(default_theme_settings());
            file.browser = Some(default_browser_settings());
            self.write_profile_file(&profile.path, &file)?;
            profile.path.clone()
        };
        self.read_profile(&path)
    }

    fn read_profile(&self, path: &Path) -> Result<Profile, String> {
        let mut file = self.read_profile_file(path)?;
        if let Some(base_profile) = file.base_profile.as_mut() {
            *base_profile = expand_home_path(base_profile);
        }
        if let Some(sidebar_locations) = file.sidebar_locations.as_mut() {
            for location in sidebar_locations {
                location.path = expand_home_path(&location.path);
            }
        }
        if let Some(browser) = file.browser.as_mut() {
            browser.thumbnail_location = expand_home_path(&browser.thumbnail_location);
        }
        let inherited = file
            .base_profile
            .as_deref()
            .map(|base| self.read_profile(base))
            .transpose()?;
        let color_mode = file
            .color_mode
            .or_else(|| inherited.as_ref().map(|profile| profile.color_mode))
            .unwrap_or_default();
        let sidebar_locations = file
            .sidebar_locations
            .or_else(|| {
                inherited
                    .as_ref()
                    .map(|profile| profile.sidebar_locations.clone())
            })
            .unwrap_or_else(default_sidebar_locations);
        let theme = file
            .theme
            .or_else(|| inherited.as_ref().map(|profile| profile.theme.clone()))
            .unwrap_or_else(default_theme_settings);
        let browser = file
            .browser
            .or_else(|| inherited.as_ref().map(|profile| profile.browser.clone()))
            .unwrap_or_else(default_browser_settings);
        let name = file.name.unwrap_or_else(|| profile_name_from_path(path));
        let read_only = fs::metadata(path)
            .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?
            .permissions()
            .readonly();
        Ok(Profile {
            path: path.to_path_buf(),
            name,
            color_mode,
            sidebar_locations,
            theme,
            browser,
            read_only,
            base_profile: file.base_profile,
        })
    }

    fn read_profile_file(&self, path: &Path) -> Result<ProfileFile, String> {
        let content = fs::read_to_string(path)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        toml::from_str(&content)
            .map_err(|error| format!("Could not parse {}: {error}", path.display()))
    }

    fn write_profile_file(&self, path: &Path, profile: &ProfileFile) -> Result<(), String> {
        let content = toml::to_string_pretty(profile)
            .map_err(|error| format!("Could not encode profile: {error}"))?;
        self.write_file(path, content)
    }

    fn write_state(&self, state: &StateFile) -> Result<(), String> {
        let content = toml::to_string_pretty(state)
            .map_err(|error| format!("Could not encode config state: {error}"))?;
        self.write_file(&self.user_config_dir.join(STATE_FILE), content)
    }

    fn write_file(&self, path: &Path, content: String) -> Result<(), String> {
        let parent = path
            .parent()
            .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
        fs::write(path, content)
            .map_err(|error| format!("Could not write {}: {error}", path.display()))
    }

    fn user_profiles_dir(&self) -> PathBuf {
        self.user_config_dir.join(PROFILES_DIRECTORY)
    }

    fn overlay_path(&self, source: &Path) -> PathBuf {
        let stem = source
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("profile");
        self.user_profiles_dir()
            .join(format!("{stem}-override-{:x}.toml", path_hash(source)))
    }
}

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

fn default_single_click_opens_folders() -> bool {
    default_browser_settings().single_click_opens_folders
}

fn default_preview_enabled() -> bool {
    default_browser_settings().preview_enabled
}

fn default_terminal_command() -> String {
    default_browser_settings().terminal_command
}

fn default_sidebar_width() -> u16 {
    default_browser_settings().sidebar_width
}

fn default_icon_theme() -> String {
    default_browser_settings().icon_theme
}

fn default_thumbnail_location() -> PathBuf {
    default_browser_settings().thumbnail_location
}

fn default_profile_file() -> ProfileFile {
    toml::from_str(DEFAULT_PROFILE_TOML).expect("default profile must be valid TOML")
}

fn expand_home_path(path: &Path) -> PathBuf {
    let Some(relative) = path.to_str().and_then(|path| path.strip_prefix("~/")) else {
        return path.to_path_buf();
    };
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(relative)
}

fn profile_filename(name: &str) -> Result<String, String> {
    let slug = name
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character.to_ascii_lowercase(),
            ' ' => '-',
            _ => '-',
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    if slug.is_empty() {
        return Err("Profile name must contain a letter or number".into());
    }
    Ok(format!("{slug}.toml"))
}

fn profile_name_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Unnamed profile")
        .into()
}

fn path_hash(path: &Path) -> u64 {
    path.as_os_str()
        .to_string_lossy()
        .bytes()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static NEXT_TEMP_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    fn test_directory() -> PathBuf {
        let unique = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = env::temp_dir().join(format!(
            "iron-file-config-test-{}-{unique}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn creates_profiles_and_persists_the_active_profile() {
        let directory = test_directory();
        let store = ConfigStore::with_paths(directory.join("user"), vec![]);
        let profile = store.create_profile("Work files").unwrap();

        assert_eq!(profile.name, "Work files");
        assert_eq!(profile.color_mode, ColorMode::System);
        assert!(profile.path.ends_with("work-files.toml"));

        store.set_active_profile(&profile.path).unwrap();
        assert_eq!(store.active_profile().unwrap(), Some(profile.path));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn discovers_profiles_from_all_search_paths() {
        let directory = test_directory();
        let user = directory.join("user");
        let system = directory.join("system");
        let store = ConfigStore::with_paths(user.clone(), vec![system.clone()]);
        store.create_profile("Personal").unwrap();
        fs::create_dir_all(system.join(PROFILES_DIRECTORY)).unwrap();
        fs::write(
            system.join(PROFILES_DIRECTORY).join("shared.toml"),
            "name = 'Shared'\ncolor_mode = 'night'\n",
        )
        .unwrap();

        let profiles = store.profiles().unwrap();
        assert_eq!(
            profiles
                .iter()
                .map(|profile| &profile.name)
                .collect::<Vec<_>>(),
            ["Personal", "Shared"]
        );
        assert_eq!(profiles[1].color_mode, ColorMode::Night);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn saves_changes_to_a_writable_profile() {
        let directory = test_directory();
        let store = ConfigStore::with_paths(directory.join("user"), vec![]);
        let profile = store.create_profile("Editable").unwrap();

        let saved = store.save_color_mode(&profile, ColorMode::Day).unwrap();

        assert_eq!(saved.path, profile.path);
        assert_eq!(saved.color_mode, ColorMode::Day);
        assert!(
            fs::read_to_string(&profile.path)
                .unwrap()
                .contains("color_mode = \"day\"")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn saves_browser_click_mode() {
        let directory = test_directory();
        let store = ConfigStore::with_paths(directory.join("user"), vec![]);
        let profile = store.create_profile("Click mode").unwrap();
        let mut browser = profile.browser.clone();
        browser.single_click_opens_folders = true;
        browser.terminal_command = "foot".into();
        browser.sidebar_width = 240;
        browser.icon_theme = "Papirus-Dark".into();
        browser.thumbnail_location = PathBuf::from("/tmp/iron-file-thumbnails");

        let saved = store.save_browser_settings(&profile, browser).unwrap();

        assert!(saved.browser.single_click_opens_folders);
        assert_eq!(saved.browser.terminal_command, "foot");
        assert_eq!(saved.browser.sidebar_width, 240);
        assert_eq!(saved.browser.icon_theme, "Papirus-Dark");
        assert_eq!(
            saved.browser.thumbnail_location,
            PathBuf::from("/tmp/iron-file-thumbnails")
        );
        let saved_toml = fs::read_to_string(&profile.path).unwrap();
        assert!(saved_toml.contains("single_click_opens_folders = true"));
        assert!(saved_toml.contains("terminal_command = \"foot\""));
        assert!(saved_toml.contains("sidebar_width = 240"));
        assert!(saved_toml.contains("icon_theme = \"Papirus-Dark\""));
        assert!(saved_toml.contains("thumbnail_location = \"/tmp/iron-file-thumbnails\""));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn saves_sidebar_locations_in_their_selected_order() {
        let directory = test_directory();
        let store = ConfigStore::with_paths(directory.join("user"), vec![]);
        let profile = store.create_profile("Sidebar").unwrap();
        let locations = vec![
            SidebarLocation {
                label: "Projects".into(),
                path: PathBuf::from("/tmp/projects"),
            },
            SidebarLocation {
                label: "Archive".into(),
                path: PathBuf::from("/tmp/archive"),
            },
        ];

        let saved = store
            .save_sidebar_locations(&profile, locations.clone())
            .unwrap();

        assert_eq!(saved.sidebar_locations, locations);
        assert!(
            fs::read_to_string(&profile.path)
                .unwrap()
                .contains("[[sidebar_locations]]")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn resets_a_profile_from_the_repository_default() {
        let directory = test_directory();
        let store = ConfigStore::with_paths(directory.join("user"), vec![]);
        let profile = store.create_profile("Resettable").unwrap();
        let changed = store.save_color_mode(&profile, ColorMode::Night).unwrap();
        let changed = store
            .save_sidebar_locations(
                &changed,
                vec![SidebarLocation {
                    label: "Other".into(),
                    path: PathBuf::from("/tmp/other"),
                }],
            )
            .unwrap();

        let reset = store.reset_profile(&changed).unwrap();

        assert_eq!(reset.color_mode, ColorMode::System);
        assert_eq!(reset.sidebar_locations, default_sidebar_locations());
        assert_eq!(reset.theme, default_theme_settings());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn editing_a_read_only_profile_creates_an_overlay() {
        let directory = test_directory();
        let user = directory.join("user");
        let system = directory.join("system");
        let store = ConfigStore::with_paths(user.clone(), vec![system.clone()]);
        let source = system.join(PROFILES_DIRECTORY).join("locked.toml");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, "name = 'Locked'\ncolor_mode = 'day'\n").unwrap();
        let mut permissions = fs::metadata(&source).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&source, permissions).unwrap();

        let locked = store.profiles().unwrap().pop().unwrap();
        assert!(locked.read_only);
        let overlay = store.save_color_mode(&locked, ColorMode::Night).unwrap();

        assert!(!overlay.read_only);
        assert_eq!(overlay.color_mode, ColorMode::Night);
        assert_eq!(overlay.base_profile, Some(source.clone()));
        let updated_overlay = store.save_color_mode(&overlay, ColorMode::System).unwrap();
        assert_eq!(updated_overlay.color_mode, ColorMode::System);
        assert_eq!(updated_overlay.base_profile, Some(source.clone()));
        assert_eq!(
            fs::read_to_string(source).unwrap(),
            "name = 'Locked'\ncolor_mode = 'day'\n"
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
