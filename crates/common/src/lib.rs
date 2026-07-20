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

use proto::{
    CreateEntryRequest, DeleteEntriesRequest, FileCommandRequest, ListDirectoryRequest,
    LogStreamRequest, OpenPathRequest, ThumbnailRequest, file_browser_client::FileBrowserClient,
};

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
    browse_with_thumbnails(path, None).await
}

pub async fn browse_with_thumbnails(
    path: PathBuf,
    thumbnail_directory: Option<PathBuf>,
) -> Result<proto::BrowseResponse, String> {
    let mut client = connect_or_start().await?;
    client
        .open_path(Request::new(OpenPathRequest {
            path: path.display().to_string(),
            thumbnail_directory: thumbnail_directory
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        }))
        .await
        .map(|response| response.into_inner())
        .map_err(|error| error.to_string())
}

pub async fn create_thumbnail(
    path: PathBuf,
    thumbnail_directory: PathBuf,
) -> Result<String, String> {
    let mut client = connect_or_start().await?;
    client
        .create_thumbnail(Request::new(ThumbnailRequest {
            path: path.display().to_string(),
            thumbnail_directory: thumbnail_directory.display().to_string(),
        }))
        .await
        .map(|response| response.into_inner().thumbnail_path)
        .map_err(|error| error.to_string())
}

pub async fn copy_entries(
    sources: Vec<PathBuf>,
    destination: PathBuf,
) -> Result<Vec<PathBuf>, String> {
    let request = FileCommandRequest {
        sources: sources
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        destination: destination.display().to_string(),
        compression_level: 0,
        compression_type: String::new(),
    };
    let mut client = connect_or_start().await?;
    match client.copy_entries(Request::new(request.clone())).await {
        Ok(response) => Ok(response
            .into_inner()
            .copied_paths
            .into_iter()
            .map(PathBuf::from)
            .collect()),
        Err(error) if error.code() == tonic::Code::Unimplemented => {
            // A development backend can outlive a frontend rebuild. Restart it once so its
            // gRPC service definition matches the frontend before reporting an error.
            restart_backend().await?;
            let mut client = connect_or_start().await?;
            client
                .copy_entries(Request::new(request))
                .await
                .map(|response| {
                    response
                        .into_inner()
                        .copied_paths
                        .into_iter()
                        .map(PathBuf::from)
                        .collect()
                })
                .map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

pub async fn delete_entries(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    let mut client = connect_or_start().await?;
    client
        .delete_entries(Request::new(DeleteEntriesRequest {
            paths: paths
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
        }))
        .await
        .map(|response| {
            response
                .into_inner()
                .copied_paths
                .into_iter()
                .map(PathBuf::from)
                .collect()
        })
        .map_err(|error| error.to_string())
}

pub async fn create_symlinks(
    sources: Vec<PathBuf>,
    destination: PathBuf,
) -> Result<Vec<PathBuf>, String> {
    let mut client = connect_or_start().await?;
    client
        .create_symlinks(Request::new(FileCommandRequest {
            sources: sources
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            destination: destination.display().to_string(),
            compression_level: 0,
            compression_type: String::new(),
        }))
        .await
        .map(|response| {
            response
                .into_inner()
                .copied_paths
                .into_iter()
                .map(PathBuf::from)
                .collect()
        })
        .map_err(|error| error.to_string())
}

pub async fn create_entry(
    parent: PathBuf,
    name: String,
    is_directory: bool,
) -> Result<PathBuf, String> {
    let mut client = connect_or_start().await?;
    client
        .create_entry(Request::new(CreateEntryRequest {
            parent: parent.display().to_string(),
            name,
            is_directory,
        }))
        .await
        .map(|response| {
            response
                .into_inner()
                .copied_paths
                .into_iter()
                .next()
                .map(PathBuf::from)
                .unwrap_or_default()
        })
        .map_err(|error| error.to_string())
}

pub async fn compress_entries(
    sources: Vec<PathBuf>,
    destination: PathBuf,
    compression_level: i32,
    compression_type: String,
) -> Result<Vec<PathBuf>, String> {
    let mut client = connect_or_start().await?;
    client
        .compress_entries(Request::new(FileCommandRequest {
            sources: sources
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            destination: destination.display().to_string(),
            compression_level,
            compression_type,
        }))
        .await
        .map(|response| {
            response
                .into_inner()
                .copied_paths
                .into_iter()
                .map(PathBuf::from)
                .collect()
        })
        .map_err(|error| error.to_string())
}

pub async fn extract_archives(
    sources: Vec<PathBuf>,
    destination: PathBuf,
) -> Result<Vec<PathBuf>, String> {
    let mut client = connect_or_start().await?;
    client
        .extract_archives(Request::new(FileCommandRequest {
            sources: sources
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            destination: destination.display().to_string(),
            compression_level: 0,
            compression_type: String::new(),
        }))
        .await
        .map(|response| {
            response
                .into_inner()
                .copied_paths
                .into_iter()
                .map(PathBuf::from)
                .collect()
        })
        .map_err(|error| error.to_string())
}

pub fn stream_directory(
    path: PathBuf,
) -> impl tokio_stream::Stream<Item = Result<proto::FileEntry, String>> {
    async_stream::stream! {
        let mut client = match connect_or_start().await {
            Ok(client) => client,
            Err(error) => {
                yield Err(error);
                return;
            }
        };
        let mut entries = match client
            .list_directory(Request::new(ListDirectoryRequest {
                path: path.display().to_string(),
            }))
            .await
        {
            Ok(response) => response.into_inner(),
            Err(error) => {
                yield Err(error.to_string());
                return;
            }
        };
        loop {
            match entries.message().await {
                Ok(Some(entry)) => yield Ok(entry),
                Ok(None) => break,
                Err(error) => {
                    yield Err(error.to_string());
                    break;
                }
            }
        }
    }
}

pub async fn ensure_backend() -> Result<(), String> {
    connect_or_start().await.map(|_| ())
}

pub async fn restart_backend() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("pkill")
            .args(["-f", "iron-file-backend"])
            .status();
        sleep(Duration::from_millis(100)).await;
    }
    #[cfg(not(target_os = "linux"))]
    {
        return Err("Restarting the backend is not supported on this platform".into());
    }
    ensure_backend().await
}

pub async fn pipe_backend_logs() -> Result<(), String> {
    loop {
        let mut client = match connect_or_start().await {
            Ok(client) => client,
            Err(error) => {
                eprintln!("[iron-file backend] log connection failed: {error}");
                sleep(STARTUP_RETRY_DELAY).await;
                continue;
            }
        };
        let mut logs = match client.stream_logs(Request::new(LogStreamRequest {})).await {
            Ok(response) => response.into_inner(),
            Err(error) => {
                eprintln!("[iron-file backend] log subscription failed: {error}");
                sleep(STARTUP_RETRY_DELAY).await;
                continue;
            }
        };
        loop {
            match logs.message().await {
                Ok(Some(entry)) => println!("[iron-file backend] {}", entry.message),
                Ok(None) => break,
                Err(error) => {
                    eprintln!("[iron-file backend] log stream failed: {error}");
                    break;
                }
            }
        }
        sleep(STARTUP_RETRY_DELAY).await;
    }
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
