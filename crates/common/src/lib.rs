use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

pub mod config;

use fs2::FileExt;
use hyper_util::rt::TokioIo;
use tokio::{net::UnixStream, time::sleep};
use tonic::{
    Request,
    transport::{Endpoint, Uri},
};
use tower::service_fn;

pub mod proto {
    tonic::include_proto!("ironfile.v1");
}

use proto::{OpenPathRequest, file_browser_client::FileBrowserClient};

const BACKEND_MODE_ENV: &str = "IRON_FILE_BACKEND_MODE";
const BACKEND_BIN_ENV: &str = "IRON_FILE_BACKEND_BIN";
const STARTUP_ATTEMPTS: usize = 50;
const STARTUP_RETRY_DELAY: Duration = Duration::from_millis(100);

pub fn socket_path() -> PathBuf {
    std::env::var_os("IRON_FILE_SOCKET")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_RUNTIME_DIR")
                .map(|dir| PathBuf::from(dir).join("iron-file-backend.sock"))
        })
        .unwrap_or_else(|| std::env::temp_dir().join("iron-file-backend.sock"))
}

pub fn backend_lock_path(socket: &Path) -> PathBuf {
    append_suffix(socket, ".lock")
}

fn startup_lock_path(socket: &Path) -> PathBuf {
    append_suffix(socket, ".startup.lock")
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

pub async fn browse(path: PathBuf) -> Result<proto::BrowseResponse, String> {
    let mut client = connect_or_start().await?;
    client
        .open_path(Request::new(OpenPathRequest {
            path: path.display().to_string(),
        }))
        .await
        .map(|response| response.into_inner())
        .map_err(|error| error.to_string())
}

pub async fn ensure_backend() -> Result<(), String> {
    connect_or_start().await.map(|_| ())
}

async fn connect_or_start() -> Result<FileBrowserClient<tonic::transport::Channel>, String> {
    if let Ok(client) = connect().await {
        return Ok(client);
    }

    let socket = socket_path();
    let _startup_lock = acquire_startup_lock(&socket).await?;

    if let Ok(client) = connect().await {
        return Ok(client);
    }

    start_backend(&socket)?;
    for _ in 0..STARTUP_ATTEMPTS {
        match connect().await {
            Ok(client) => return Ok(client),
            Err(_) => sleep(STARTUP_RETRY_DELAY).await,
        }
    }

    Err(format!(
        "The backend did not start within {} ms",
        STARTUP_ATTEMPTS as u128 * STARTUP_RETRY_DELAY.as_millis()
    ))
}

async fn connect() -> Result<FileBrowserClient<tonic::transport::Channel>, String> {
    let socket = socket_path();
    let endpoint = Endpoint::try_from("http://[::]:50051").map_err(|error| error.to_string())?;
    let channel = endpoint
        .connect_with_connector(service_fn(move |_: Uri| {
            let socket = socket.clone();
            async move { UnixStream::connect(socket).await.map(TokioIo::new) }
        }))
        .await
        .map_err(|error| error.to_string())?;
    Ok(FileBrowserClient::new(channel))
}

async fn acquire_startup_lock(socket: &Path) -> Result<File, String> {
    let lock_path = startup_lock_path(socket);
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|error| error.to_string())?;

    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(file),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                sleep(STARTUP_RETRY_DELAY).await
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn start_backend(socket: &Path) -> Result<(), String> {
    let mode = std::env::var(BACKEND_MODE_ENV).unwrap_or_else(|_| "dev".into());
    let mut command = match mode.as_str() {
        "dev" => {
            let mut command = Command::new("cargo");
            command.args([
                "run",
                "--manifest-path",
                workspace_manifest().to_string_lossy().as_ref(),
                "--package",
                "iron-file-backend",
            ]);
            command
        }
        "prod" => {
            let path = std::env::var_os(BACKEND_BIN_ENV).ok_or_else(|| {
                format!("{BACKEND_BIN_ENV} must point to the backend binary in prod mode")
            })?;
            Command::new(path)
        }
        _ => {
            return Err(format!("{BACKEND_MODE_ENV} must be either dev or prod"));
        }
    };

    command
        .env("IRON_FILE_SOCKET", socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Failed to start backend: {error}"))
}

fn workspace_manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml")
}
