use super::*;

pub(super) struct StartupOptions {
    pub(super) follow_logs: bool,
    pub(super) initial_path: Option<PathBuf>,
    pub(super) picker: Option<PickerOptions>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PickerKind {
    File,
    Folder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PickerOptions {
    pub(super) kind: PickerKind,
    pub(super) multiple: bool,
    pub(super) save_file_name: Option<String>,
}

pub(super) fn startup_options() -> StartupOptions {
    parse_startup_options(env::args_os().skip(1))
}

pub(super) fn parse_startup_options(
    arguments: impl IntoIterator<Item = OsString>,
) -> StartupOptions {
    let mut options = StartupOptions {
        follow_logs: false,
        initial_path: None,
        picker: None,
    };
    let mut picker_kind = PickerKind::File;
    let mut picker_multiple = false;
    let mut picker_requested = false;
    let mut save_file_name = None;

    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "-f" || argument == "--follow" {
            options.follow_logs = true;
        } else if argument == "--mode" {
            picker_requested = arguments.next().is_some_and(|mode| mode == "picker");
        } else if argument == "--picker" {
            picker_requested = true;
        } else if argument == "--file" {
            picker_kind = PickerKind::File;
        } else if argument == "--folder" {
            picker_kind = PickerKind::Folder;
        } else if argument == "--multiple" || argument == "--multi" {
            picker_multiple = true;
        } else if argument == "--single" {
            picker_multiple = false;
        } else if argument == "--save-name" {
            save_file_name = arguments.next().and_then(|name| name.into_string().ok());
        } else if !argument.to_string_lossy().starts_with('-') && options.initial_path.is_none() {
            options.initial_path = Some(PathBuf::from(argument));
        }
    }

    options.picker = picker_requested.then_some(PickerOptions {
        kind: picker_kind,
        multiple: picker_multiple,
        save_file_name,
    });

    options
}

pub(super) fn resolve_initial_path(path: PathBuf) -> PathBuf {
    let current_directory = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    resolve_path_from(path, current_directory)
}

pub(super) fn resolve_path_from(path: PathBuf, current_directory: PathBuf) -> PathBuf {
    let path = if path.is_absolute() {
        path
    } else {
        current_directory.join(path)
    };
    fs::canonicalize(&path).unwrap_or(path)
}

pub(super) fn valid_save_file_name(name: &str) -> bool {
    !name.is_empty()
        && Path::new(name)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

pub(super) fn is_detached() -> bool {
    env::var_os(DETACHED_ENV).is_some()
}

pub(super) fn detach() -> std::io::Result<()> {
    Command::new(env::current_exe()?)
        .args(env::args_os().skip(1))
        .env(DETACHED_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

pub(super) fn clone_window(path: &Path) -> Result<(), String> {
    Command::new(env::current_exe().map_err(|error| error.to_string())?)
        .arg(path)
        .env(DETACHED_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open a new window: {error}"))
}

#[cfg(test)]
mod startup_tests {
    use super::*;

    #[test]
    fn accepts_a_positional_location_and_follow_flag() {
        let options =
            parse_startup_options(["-f", "/tmp/first", "/tmp/second"].map(OsString::from));

        assert!(options.follow_logs);
        assert_eq!(options.initial_path, Some(PathBuf::from("/tmp/first")));
    }

    #[test]
    fn accepts_a_location_before_the_follow_flag() {
        let options = parse_startup_options(["/tmp/first", "--follow"].map(OsString::from));

        assert!(options.follow_logs);
        assert_eq!(options.initial_path, Some(PathBuf::from("/tmp/first")));
    }

    #[test]
    fn parses_folder_multi_picker_mode() {
        let options = parse_startup_options(
            ["--mode", "picker", "--folder", "--multiple"].map(OsString::from),
        );

        assert_eq!(
            options.picker,
            Some(PickerOptions {
                kind: PickerKind::Folder,
                multiple: true,
                save_file_name: None,
            })
        );
    }

    #[test]
    fn parses_a_save_name_for_picker_mode() {
        let options = parse_startup_options(
            ["--mode", "picker", "--folder", "--save-name", "report.txt"].map(OsString::from),
        );

        assert_eq!(
            options.picker.and_then(|picker| picker.save_file_name),
            Some("report.txt".into())
        );
    }

    #[test]
    fn save_name_cannot_escape_the_selected_folder() {
        assert!(valid_save_file_name("report.txt"));
        assert!(!valid_save_file_name("../report.txt"));
        assert!(!valid_save_file_name("/tmp/report.txt"));
    }

    #[test]
    fn accepts_current_and_relative_locations() {
        let current = parse_startup_options(["."].map(OsString::from));
        let relative = parse_startup_options(["./Documents"].map(OsString::from));

        assert_eq!(current.initial_path, Some(PathBuf::from(".")));
        assert_eq!(relative.initial_path, Some(PathBuf::from("./Documents")));
    }

    #[test]
    fn resolves_relative_locations_from_the_invoking_directory() {
        let location = resolve_path_from(PathBuf::from("."), PathBuf::from("/tmp"));

        assert_eq!(location, PathBuf::from("/tmp"));
    }
}
