use std::{
    collections::HashMap,
    env, fs,
    os::fd::AsRawFd,
    path::{Component, Path, PathBuf},
    process::Stdio,
};

use tokio::process::Command;
use url::Url;
use zbus::{
    fdo,
    zvariant::{OwnedFd, OwnedObjectPath, OwnedValue, Value},
};

const BUS_NAME: &str = "org.freedesktop.impl.portal.desktop.iron-file";
const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";
const FILE_MANAGER_BUS_NAME: &str = "org.freedesktop.FileManager1";
const FILE_MANAGER_OBJECT_PATH: &str = "/org/freedesktop/FileManager1";

struct FileChooser {
    executable: PathBuf,
}

struct OpenUri {
    executable: PathBuf,
}

struct FileManager1 {
    executable: PathBuf,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.OpenURI")]
impl OpenUri {
    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }

    async fn open_file(
        &self,
        _handle: OwnedObjectPath,
        _app_id: String,
        _parent_window: String,
        fd: OwnedFd,
        _options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<(u32, HashMap<String, OwnedValue>)> {
        let path = path_from_fd(&fd)?;
        launch_associated_application(&path).await?;
        Ok((0, HashMap::new()))
    }

    async fn open_directory(
        &self,
        _handle: OwnedObjectPath,
        _app_id: String,
        _parent_window: String,
        fd: OwnedFd,
        _options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<(u32, HashMap<String, OwnedValue>)> {
        let path = path_from_fd(&fd)?;
        let directory = if path.is_dir() {
            path
        } else {
            path.parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| fdo::Error::Failed("file has no parent directory".into()))?
        };
        launch_file_manager(&self.executable, &directory).await?;
        Ok((0, HashMap::new()))
    }
}

#[zbus::interface(name = "org.freedesktop.FileManager1")]
impl FileManager1 {
    async fn show_folders(&self, uris: Vec<String>, _startup_id: String) -> fdo::Result<()> {
        for directory in unique(paths_from_uris(&uris)) {
            launch_file_manager(&self.executable, &directory).await?;
        }
        Ok(())
    }

    async fn show_items(&self, uris: Vec<String>, _startup_id: String) -> fdo::Result<()> {
        let directories = paths_from_uris(&uris)
            .into_iter()
            .map(|path| parent_directory(&path))
            .collect::<Vec<_>>();
        for directory in unique(directories) {
            launch_file_manager(&self.executable, &directory).await?;
        }
        Ok(())
    }

    // Iron File has no item-properties dialog, so this opens the containing
    // folder as a best-effort fallback rather than failing the request.
    async fn show_item_properties(
        &self,
        uris: Vec<String>,
        _startup_id: String,
    ) -> fdo::Result<()> {
        self.show_items(uris, _startup_id).await
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl FileChooser {
    #[zbus(property)]
    fn version(&self) -> u32 {
        1
    }

    async fn open_file(
        &self,
        _handle: OwnedObjectPath,
        _app_id: String,
        _parent_window: String,
        _title: String,
        options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<(u32, HashMap<String, OwnedValue>)> {
        let directory = option_bool(&options, "directory");
        let multiple = option_bool(&options, "multiple");
        let paths = self.select_locations(directory, multiple, None).await?;
        if paths.is_empty() {
            return Ok((1, HashMap::new()));
        }

        response(paths)
    }

    async fn save_file(
        &self,
        _handle: OwnedObjectPath,
        _app_id: String,
        _parent_window: String,
        _title: String,
        options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<(u32, HashMap<String, OwnedValue>)> {
        let name = save_file_name(&options)?;
        let Some(path) = self
            .select_locations(true, false, Some(&name))
            .await?
            .into_iter()
            .next()
        else {
            return Ok((1, HashMap::new()));
        };
        response(vec![path])
    }

    async fn save_files(
        &self,
        _handle: OwnedObjectPath,
        _app_id: String,
        _parent_window: String,
        _title: String,
        options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<(u32, HashMap<String, OwnedValue>)> {
        let Some(folder) = self
            .select_locations(true, false, None)
            .await?
            .into_iter()
            .next()
        else {
            return Ok((1, HashMap::new()));
        };
        let names = save_file_names(&options)?;
        response(names.into_iter().map(|name| folder.join(name)).collect())
    }
}

impl FileChooser {
    async fn select_locations(
        &self,
        directory: bool,
        multiple: bool,
        save_name: Option<&str>,
    ) -> fdo::Result<Vec<PathBuf>> {
        let mut command = Command::new(&self.executable);
        command.args(["--mode", "picker"]);
        command.arg(if directory { "--folder" } else { "--file" });
        command.arg(if multiple { "--multiple" } else { "--single" });
        if let Some(save_name) = save_name {
            command.args(["--save-name", save_name]);
        }
        command.stdout(Stdio::piped()).stderr(Stdio::null());

        let output = command
            .output()
            .await
            .map_err(|error| fdo::Error::Failed(error.to_string()))?;
        if !output.status.success() {
            return Ok(Vec::new());
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(PathBuf::from)
            .collect())
    }
}

fn path_from_fd(fd: &OwnedFd) -> fdo::Result<PathBuf> {
    fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd()))
        .map_err(|error| fdo::Error::Failed(format!("could not resolve file descriptor: {error}")))
}

async fn launch_associated_application(path: &Path) -> fdo::Result<()> {
    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| fdo::Error::Failed(format!("could not open file: {error}")))
}

fn paths_from_uris(uris: &[String]) -> Vec<PathBuf> {
    uris.iter()
        .filter_map(|uri| Url::parse(uri).ok())
        .filter_map(|url| url.to_file_path().ok())
        .collect()
}

fn parent_directory(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.to_path_buf())
    }
}

fn unique(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

async fn launch_file_manager(executable: &Path, directory: &Path) -> fdo::Result<()> {
    Command::new(executable)
        .arg(directory)
        .spawn()
        .map(|_| ())
        .map_err(|error| fdo::Error::Failed(format!("could not open directory: {error}")))
}

fn response(paths: Vec<PathBuf>) -> fdo::Result<(u32, HashMap<String, OwnedValue>)> {
    let uris = paths
        .iter()
        .filter_map(|path| Url::from_file_path(path).ok())
        .map(|uri| uri.into())
        .collect::<Vec<String>>();
    if uris.len() != paths.len() || uris.is_empty() {
        return Err(fdo::Error::Failed(
            "could not convert selected paths to file URIs".into(),
        ));
    }
    let uris = OwnedValue::try_from(Value::from(uris))
        .map_err(|error| fdo::Error::Failed(error.to_string()))?;
    Ok((0, HashMap::from([("uris".into(), uris)])))
}

fn save_file_name(options: &HashMap<String, OwnedValue>) -> fdo::Result<String> {
    options
        .get("current_name")
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| String::try_from(value).ok())
        .filter(|name| valid_file_name(name))
        .ok_or_else(|| fdo::Error::InvalidArgs("SaveFile requires a current_name".into()))
}

fn save_file_names(options: &HashMap<String, OwnedValue>) -> fdo::Result<Vec<String>> {
    let names = options
        .get("files")
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| Vec::<Vec<u8>>::try_from(value).ok())
        .map(|names| {
            names
                .into_iter()
                .filter_map(|name| String::from_utf8(name).ok())
                .map(|name| name.trim_end_matches('\0').to_owned())
                .collect::<Vec<_>>()
        })
        .filter(|names| !names.is_empty() && names.iter().all(|name| valid_file_name(name)));
    names.ok_or_else(|| fdo::Error::InvalidArgs("SaveFiles requires valid file names".into()))
}

fn valid_file_name(name: &str) -> bool {
    !name.is_empty()
        && Path::new(name)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::{parent_directory, path_from_fd, paths_from_uris, unique, valid_file_name};
    use std::{fs::File, os::fd::OwnedFd, path::PathBuf};
    use zbus::zvariant::OwnedFd as ZbusOwnedFd;

    #[test]
    fn save_names_must_be_simple_file_names() {
        assert!(valid_file_name("report.txt"));
        assert!(!valid_file_name(""));
        assert!(!valid_file_name("../report.txt"));
        assert!(!valid_file_name("/tmp/report.txt"));
    }

    #[test]
    fn resolves_paths_from_file_descriptors() {
        let file = File::open("/dev/null").unwrap();
        let fd: OwnedFd = file.into();
        let fd: ZbusOwnedFd = fd.into();

        assert_eq!(path_from_fd(&fd).unwrap(), PathBuf::from("/dev/null"));
    }

    #[test]
    fn resolves_paths_from_file_uris_and_ignores_others() {
        let paths = paths_from_uris(&[
            "file:///dev/null".to_owned(),
            "not a uri".to_owned(),
            "http://example.com/file".to_owned(),
        ]);
        assert_eq!(paths, vec![PathBuf::from("/dev/null")]);
    }

    #[test]
    fn parent_directory_of_a_file_is_its_parent() {
        assert_eq!(
            parent_directory(&PathBuf::from("/dev/null")),
            PathBuf::from("/dev")
        );
    }

    #[test]
    fn parent_directory_of_a_directory_is_itself() {
        assert_eq!(
            parent_directory(&PathBuf::from("/dev")),
            PathBuf::from("/dev")
        );
    }

    #[test]
    fn unique_removes_duplicate_paths_preserving_order() {
        let paths = unique(vec![
            PathBuf::from("/a"),
            PathBuf::from("/b"),
            PathBuf::from("/a"),
        ]);
        assert_eq!(paths, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }
}

fn option_bool(options: &HashMap<String, OwnedValue>, key: &str) -> bool {
    options
        .get(key)
        .and_then(|value| bool::try_from(value).ok())
        .unwrap_or(false)
}

#[tokio::main]
async fn main() -> zbus::Result<()> {
    let executable = env::var_os("IRON_FILE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("iron-file-iced"));
    let connection = zbus::ConnectionBuilder::session()?
        .name(BUS_NAME)?
        .serve_at(
            OBJECT_PATH,
            FileChooser {
                executable: executable.clone(),
            },
        )?
        .serve_at(
            OBJECT_PATH,
            OpenUri {
                executable: executable.clone(),
            },
        )?
        .serve_at(FILE_MANAGER_OBJECT_PATH, FileManager1 { executable })?
        .build()
        .await?;

    // Best-effort: another file manager (e.g. Nautilus) may already own this
    // name, in which case the portal backend above still starts normally.
    if let Err(error) = connection.request_name(FILE_MANAGER_BUS_NAME).await {
        eprintln!("could not acquire {FILE_MANAGER_BUS_NAME}: {error}");
    }
    std::future::pending::<()>().await;
    Ok(())
}
