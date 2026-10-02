use super::*;

#[derive(Debug, Clone)]
pub struct ConfigStore {
    user_config_dir: PathBuf,
    search_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub(super) struct ProfileFile {
    pub(super) name: Option<String>,
    pub(super) color_mode: Option<ColorMode>,
    pub(super) sidebar_locations: Option<Vec<SidebarLocation>>,
    pub(super) theme: Option<ThemeSettings>,
    pub(super) browser: Option<BrowserSettings>,
    pub(super) base_profile: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub(super) struct StateFile {
    pub(super) active_profile: Option<PathBuf>,
    #[serde(default)]
    pub(super) last_picker_directory: Option<PathBuf>,
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
        Ok(self
            .read_state()?
            .active_profile
            .filter(|profile| profile.exists()))
    }

    pub fn set_active_profile(&self, profile: &Path) -> Result<(), String> {
        let mut state = self.read_state()?;
        state.active_profile = Some(profile.to_path_buf());
        self.write_state(&state)
    }

    pub fn last_picker_directory(&self) -> Result<Option<PathBuf>, String> {
        Ok(self
            .read_state()?
            .last_picker_directory
            .filter(|directory| directory.is_dir()))
    }

    pub fn set_last_picker_directory(&self, directory: &Path) -> Result<(), String> {
        let mut state = self.read_state()?;
        state.last_picker_directory = Some(directory.to_path_buf());
        self.write_state(&state)
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

    pub fn save_theme_settings(
        &self,
        profile: &Profile,
        theme: ThemeSettings,
    ) -> Result<Profile, String> {
        if profile.read_only {
            let overlay = self.overlay_path(&profile.path);
            let file = ProfileFile {
                name: Some(profile.name.clone()),
                color_mode: None,
                sidebar_locations: None,
                theme: Some(theme),
                browser: None,
                base_profile: Some(profile.path.clone()),
            };
            self.write_profile_file(&overlay, &file)?;
            return self.read_profile(&overlay);
        }

        let mut file = self.read_profile_file(&profile.path)?;
        file.theme = Some(theme);
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

    pub(super) fn read_profile(&self, path: &Path) -> Result<Profile, String> {
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
            browser.apply_legacy_context_menu_items();
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

    pub(super) fn read_profile_file(&self, path: &Path) -> Result<ProfileFile, String> {
        let content = fs::read_to_string(path)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        toml::from_str(&content)
            .map_err(|error| format!("Could not parse {}: {error}", path.display()))
    }

    pub(super) fn write_profile_file(
        &self,
        path: &Path,
        profile: &ProfileFile,
    ) -> Result<(), String> {
        let content = toml::to_string_pretty(profile)
            .map_err(|error| format!("Could not encode profile: {error}"))?;
        self.write_file(path, content)
    }

    pub(super) fn read_state(&self) -> Result<StateFile, String> {
        let path = self.user_config_dir.join(STATE_FILE);
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(StateFile::default());
            }
            Err(error) => return Err(format!("Could not read {}: {error}", path.display())),
        };
        toml::from_str::<StateFile>(&content)
            .map_err(|error| format!("Could not parse {}: {error}", path.display()))
    }

    pub(super) fn write_state(&self, state: &StateFile) -> Result<(), String> {
        let content = toml::to_string_pretty(state)
            .map_err(|error| format!("Could not encode config state: {error}"))?;
        self.write_file(&self.user_config_dir.join(STATE_FILE), content)
    }

    pub(super) fn write_file(&self, path: &Path, content: String) -> Result<(), String> {
        let parent = path
            .parent()
            .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
        fs::write(path, content)
            .map_err(|error| format!("Could not write {}: {error}", path.display()))
    }

    pub(super) fn user_profiles_dir(&self) -> PathBuf {
        self.user_config_dir.join(PROFILES_DIRECTORY)
    }

    pub(super) fn overlay_path(&self, source: &Path) -> PathBuf {
        let stem = source
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("profile");
        self.user_profiles_dir()
            .join(format!("{stem}-override-{:x}.toml", path_hash(source)))
    }
}

pub(super) fn default_profile_file() -> ProfileFile {
    toml::from_str(DEFAULT_PROFILE_TOML).expect("default profile must be valid TOML")
}

pub(super) fn expand_home_path(path: &Path) -> PathBuf {
    let Some(relative) = path.to_str().and_then(|path| path.strip_prefix("~/")) else {
        return path.to_path_buf();
    };
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(relative)
}

pub(super) fn profile_filename(name: &str) -> Result<String, String> {
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

pub(super) fn profile_name_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Unnamed profile")
        .into()
}

pub(super) fn path_hash(path: &Path) -> u64 {
    path.as_os_str()
        .to_string_lossy()
        .bytes()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
}
