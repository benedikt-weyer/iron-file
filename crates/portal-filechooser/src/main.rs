use std::{collections::HashMap, env, path::PathBuf, process::Stdio};

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
        let mut command = Command::new(&self.executable);
        command.args(["--mode", "picker"]);
        command.arg(if directory { "--folder" } else { "--file" });
        command.arg(if multiple { "--multiple" } else { "--single" });
        command.stdout(Stdio::piped()).stderr(Stdio::null());

        let output = command
            .output()
            .await
            .map_err(|error| fdo::Error::Failed(error.to_string()))?;
        if !output.status.success() {
            return Ok((1, HashMap::new()));
        }

        let uris = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| Url::from_file_path(line).ok())
            .map(|uri| uri.into())
            .collect::<Vec<String>>();
        if uris.is_empty() {
            return Ok((1, HashMap::new()));
        }
        let uris = OwnedValue::try_from(Value::from(uris))
            .map_err(|error| fdo::Error::Failed(error.to_string()))?;
        Ok((0, HashMap::from([("uris".into(), uris)])))
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
