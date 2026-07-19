use std::{
    fs::{File, OpenOptions},
    io,
    path::{Path, PathBuf},
    pin::Pin,
};

use fs2::FileExt;
use iron_file_common::{backend_lock_path, proto, socket_path};
use tokio::{
    net::{UnixListener, UnixStream},
    sync::{broadcast, mpsc},
};
use tokio_stream::{
    Stream,
    wrappers::{ReceiverStream, UnixListenerStream},
};
use tonic::{Request, Response, Status, transport::Server};

use proto::{
    BrowseResponse, BrowserError, Directory, FileContent, FileEntry, LogEntry, LogStreamRequest,
    OpenPathRequest,
    browse_response::Payload,
    file_browser_server::{FileBrowser, FileBrowserServer},
};

const MAX_PREVIEW_BYTES: u64 = 1_000_000;

#[derive(Clone)]
struct FileBrowserService {
    logs: broadcast::Sender<String>,
}

#[tonic::async_trait]
impl FileBrowser for FileBrowserService {
    type StreamLogsStream = Pin<Box<dyn Stream<Item = Result<LogEntry, Status>> + Send>>;

    async fn open_path(
        &self,
        request: Request<OpenPathRequest>,
    ) -> Result<Response<BrowseResponse>, Status> {
        let path = PathBuf::from(request.into_inner().path);
        self.log(format!("Opening {}", path.display()));
        let response = browse(path);
        if let Some(Payload::Error(error)) = &response.payload {
            self.log(format!("Request failed: {}", error.message));
        }
        Ok(Response::new(response))
    }

    async fn stream_logs(
        &self,
        _: Request<LogStreamRequest>,
    ) -> Result<Response<Self::StreamLogsStream>, Status> {
        let mut logs = self.logs.subscribe();
        let (sender, receiver) = mpsc::channel(128);
        tokio::spawn(async move {
            loop {
                match logs.recv().await {
                    Ok(message) => {
                        if sender.send(Ok(LogEntry { message })).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(count)) => {
                        let message = format!("Skipped {count} backend log messages");
                        if sender.send(Ok(LogEntry { message })).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        self.log("Frontend subscribed to backend logs");
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }
}

impl FileBrowserService {
    fn log(&self, message: impl Into<String>) {
        let message = message.into();
        eprintln!("{message}");
        let _ = self.logs.send(message);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let socket = socket_path();
    let _lock = acquire_singleton_lock(&socket)?;
    let listener = bind_singleton_socket(&socket).await?;
    let (logs, _) = broadcast::channel(256);
    let service = FileBrowserService { logs };
    service.log(format!(
        "iron-file backend listening on {}",
        socket.display()
    ));

    Server::builder()
        .add_service(FileBrowserServer::new(service))
        .serve_with_incoming(UnixListenerStream::new(listener))
        .await?;

    Ok(())
}

fn acquire_singleton_lock(socket: &Path) -> io::Result<File> {
    let lock_path = backend_lock_path(socket);
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(lock_path)?;
    file.try_lock_exclusive().map_err(|_| {
        io::Error::new(
            io::ErrorKind::AddrInUse,
            format!(
                "another iron-file backend already owns {}",
                socket.display()
            ),
        )
    })?;
    Ok(file)
}

async fn bind_singleton_socket(path: &Path) -> io::Result<UnixListener> {
    match UnixListener::bind(path) {
        Ok(listener) => Ok(listener),
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
            let _ = UnixStream::connect(path).await;
            std::fs::remove_file(path)?;
            UnixListener::bind(path)
        }
        Err(error) => Err(error),
    }
}

fn browse(path: PathBuf) -> BrowseResponse {
    let display_path = path.display().to_string();
    let payload = match std::fs::metadata(&path) {
        Ok(metadata) if metadata.is_dir() => directory_payload(&path),
        Ok(metadata) if metadata.is_file() => file_payload(&path, metadata.len()),
        Ok(_) => error_payload("Unsupported filesystem entry"),
        Err(error) => error_payload(error.to_string()),
    };

    BrowseResponse {
        path: display_path,
        payload: Some(payload),
    }
}

fn directory_payload(path: &Path) -> Payload {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => return error_payload(error.to_string()),
    };

    let mut files = entries
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            FileEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_directory: path.is_dir(),
                path: path.display().to_string(),
            }
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        right
            .is_directory
            .cmp(&left.is_directory)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    Payload::Directory(Directory { entries: files })
}

fn file_payload(path: &Path, size: u64) -> Payload {
    if size > MAX_PREVIEW_BYTES {
        return Payload::File(FileContent {
            content: format!("Preview unavailable: file is larger than {MAX_PREVIEW_BYTES} bytes."),
        });
    }

    match std::fs::read(path) {
        Ok(contents) => match String::from_utf8(contents) {
            Ok(content) => Payload::File(FileContent { content }),
            Err(_) => Payload::File(FileContent {
                content: "Preview unavailable: binary file.".into(),
            }),
        },
        Err(error) => error_payload(error.to_string()),
    }
}

fn error_payload(message: impl Into<String>) -> Payload {
    Payload::Error(BrowserError {
        message: message.into(),
    })
}
