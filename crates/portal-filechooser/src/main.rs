use std::{
    collections::HashMap,
    env,
    path::{Component, Path, PathBuf},
    process::Stdio,
};

use tokio::process::Command;
use url::Url;
use zbus::{
    fdo,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

const BUS_NAME: &str = "org.freedesktop.impl.portal.desktop.iron-file";
const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";

struct FileChooser {
    executable: PathBuf,
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
    use super::valid_file_name;

    #[test]
    fn save_names_must_be_simple_file_names() {
        assert!(valid_file_name("report.txt"));
        assert!(!valid_file_name(""));
        assert!(!valid_file_name("../report.txt"));
        assert!(!valid_file_name("/tmp/report.txt"));
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
    let _connection = zbus::ConnectionBuilder::session()?
        .name(BUS_NAME)?
        .serve_at(OBJECT_PATH, FileChooser { executable })?
        .build()
        .await?;
    std::future::pending::<()>().await;
    Ok(())
}
