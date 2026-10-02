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
fn persists_the_last_picker_directory_alongside_the_active_profile() {
    let directory = test_directory();
    let store = ConfigStore::with_paths(directory.join("user"), vec![]);
    let profile = store.create_profile("Work files").unwrap();
    store.set_active_profile(&profile.path).unwrap();

    let picker_directory = directory.join("Downloads");
    fs::create_dir_all(&picker_directory).unwrap();
    store.set_last_picker_directory(&picker_directory).unwrap();

    assert_eq!(
        store.last_picker_directory().unwrap(),
        Some(picker_directory)
    );
    assert_eq!(store.active_profile().unwrap(), Some(profile.path));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn reads_the_legacy_context_menu_blur_as_strength() {
    let profile: ProfileFile = toml::from_str(
        r##"
            [theme]
            light_highlight = "#111111"
            dark_highlight = "#222222"
            context_menu_blur = 14
        "##,
    )
    .unwrap();

    let theme = profile.theme.unwrap();
    assert_eq!(theme.context_menu_blur_strength, 14);
    assert_eq!(
        theme.context_menu_blur_kernel_size,
        default_context_menu_blur_kernel_size()
    );
}

#[test]
fn reads_and_writes_the_dynamic_context_menu_blur_kernel() {
    let profile: ProfileFile = toml::from_str(
        r##"
            [theme]
            light_highlight = "#111111"
            dark_highlight = "#222222"
            context_menu_blur_kernel_size = "6sigma+1"
        "##,
    )
    .unwrap();

    let theme = profile.theme.unwrap();
    assert_eq!(
        theme.context_menu_blur_kernel_size,
        ContextMenuBlurKernelSize::Dynamic
    );
    assert_eq!(theme.context_menu_blur_kernel_size.effective_size(2), 13);
    assert_eq!(theme.context_menu_blur_kernel_size.effective_size(5), 31);
    assert_eq!(
        toml::to_string(&theme).unwrap(),
        "light_highlight = \"#111111\"\ndark_highlight = \"#222222\"\nbackground_opacity = 100\ncontext_menu_blur_strength = 2\ncontext_menu_blur_kernel_size = \"6sigma+1\"\nborder_radius = 6\n"
    );
}

#[test]
fn reads_legacy_context_menu_items_for_both_menus() {
    let profile: ProfileFile = toml::from_str(
        r##"
            [browser]
            item_size = 32
            layout = "list"
            context_menu_items = ["copy-location", "create-file"]
        "##,
    )
    .unwrap();

    let mut browser = profile.browser.unwrap();
    browser.apply_legacy_context_menu_items();
    let expected = vec![ContextMenuItem::CopyLocation, ContextMenuItem::CreateFile];
    assert_eq!(browser.file_context_menu_items, expected);
    assert_eq!(browser.folder_context_menu_items, expected);
}

#[test]
fn reads_quick_toolbar_items_in_configured_order() {
    let profile: ProfileFile = toml::from_str(
        r##"
            [browser]
            item_size = 32
            layout = "list"
            quick_toolbar_items = ["extract-selection", "toggle-hidden-files"]
        "##,
    )
    .unwrap();

    assert_eq!(
        profile.browser.unwrap().quick_toolbar_items,
        vec![
            QuickToolbarItem::ExtractSelection,
            QuickToolbarItem::ToggleHiddenFiles,
        ]
    );
}

#[test]
fn defaults_and_reads_name_alignment() {
    assert_eq!(
        default_browser_settings().name_alignment,
        NameAlignment::Center
    );

    let profile: ProfileFile = toml::from_str(
        r##"
            [browser]
            item_size = 32
            layout = "list"
            name_alignment = "right"
        "##,
    )
    .unwrap();

    assert_eq!(
        profile.browser.unwrap().name_alignment,
        NameAlignment::Right
    );
}

#[test]
fn defaults_and_reads_scroll_step() {
    assert_eq!(default_browser_settings().scroll_step, 60);

    let profile: ProfileFile = toml::from_str(
        r##"
            [browser]
            item_size = 32
            layout = "list"
            scroll_step = 96
        "##,
    )
    .unwrap();

    assert_eq!(profile.browser.unwrap().scroll_step, 96);
}

#[test]
fn reads_folder_sort_overrides() {
    let profile: ProfileFile = toml::from_str(
        r##"
            [browser]
            item_size = 32
            layout = "list"
            folder_sort_overrides = [
              { path = "/tmp/example", sort_order = "name-descending" },
            ]
        "##,
    )
    .unwrap();

    assert_eq!(
        profile.browser.unwrap().folder_sort_overrides,
        vec![FolderSortOverride {
            path: PathBuf::from("/tmp/example"),
            sort_order: EntrySortOrder::NameDescending,
        }]
    );
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
fn saves_theme_settings_to_a_profile() {
    let directory = test_directory();
    let store = ConfigStore::with_paths(directory.join("user"), vec![]);
    let profile = store.create_profile("Accent").unwrap();
    let theme = ThemeSettings {
        light_highlight: "#112233".into(),
        dark_highlight: "#445566".into(),
        background_opacity: 75,
        context_menu_blur_strength: 3,
        context_menu_blur_kernel_size: ContextMenuBlurKernelSize::Fixed(7),
        border_radius: 4,
    };

    let saved = store.save_theme_settings(&profile, theme.clone()).unwrap();

    assert_eq!(saved.theme, theme);
    assert!(
        fs::read_to_string(&profile.path)
            .unwrap()
            .contains("light_highlight = \"#112233\"")
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
            icon: None,
        },
        SidebarLocation {
            label: "Archive".into(),
            path: PathBuf::from("/tmp/archive"),
            icon: None,
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
                icon: None,
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
