use std::{
    fs::{File, OpenOptions},
    io::{self, BufWriter},
    path::{Path, PathBuf},
    pin::Pin,
};

use fs2::FileExt;
use image::{ImageFormat, ImageReader};
use iron_file_common::{backend_lock_path, proto, socket_path};
use sha1::{Digest, Sha1};
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
    BrowseResponse, BrowserError, Directory, FileContent, FileEntry, ListDirectoryRequest,
    LogEntry, LogStreamRequest, OpenPathRequest, ThumbnailRequest, ThumbnailResponse,
    browse_response::Payload,
    file_browser_server::{FileBrowser, FileBrowserServer},
};

const MAX_PREVIEW_BYTES: u64 = 1_000_000;

#[derive(Clone)]
struct FileBrowserService {
    logs: broadcast::Sender<String>,
}

enum ThumbnailOutcome {
    Cached(PathBuf),
    Generated(PathBuf),
    NotImage,
}

#[tonic::async_trait]
impl FileBrowser for FileBrowserService {
    type ListDirectoryStream = Pin<Box<dyn Stream<Item = Result<FileEntry, Status>> + Send>>;
    type StreamLogsStream = Pin<Box<dyn Stream<Item = Result<LogEntry, Status>> + Send>>;

    async fn open_path(
        &self,
        request: Request<OpenPathRequest>,
    ) -> Result<Response<BrowseResponse>, Status> {
        let request = request.into_inner();
        let path = PathBuf::from(request.path);
        self.log(format!("Opening {}", path.display()));
        let response = browse(path);
        if let Some(Payload::Error(error)) = &response.payload {
            self.log(format!("Request failed: {}", error.message));
        }
        Ok(Response::new(response))
    }

    async fn list_directory(
        &self,
        request: Request<ListDirectoryRequest>,
    ) -> Result<Response<Self::ListDirectoryStream>, Status> {
        let path = PathBuf::from(request.into_inner().path);
        self.log(format!("Listing directory {}", path.display()));
        let entries = directory_entries(&path).map_err(Status::internal)?;
        Ok(Response::new(Box::pin(tokio_stream::iter(
            entries.into_iter().map(Ok),
        ))))
    }

    async fn create_thumbnail(
        &self,
        request: Request<ThumbnailRequest>,
    ) -> Result<Response<ThumbnailResponse>, Status> {
        let request = request.into_inner();
        let path = PathBuf::from(request.path);
        self.log(format!("Thumbnail requested for {}", path.display()));
        let thumbnail_path = match thumbnail_for(&path, Path::new(&request.thumbnail_directory)) {
            Ok(ThumbnailOutcome::Cached(thumbnail_path)) => {
                self.log(format!("Thumbnail cache hit for {}", path.display()));
                thumbnail_path.display().to_string()
            }
            Ok(ThumbnailOutcome::Generated(thumbnail_path)) => {
                self.log(format!("Thumbnail generated for {}", path.display()));
                thumbnail_path.display().to_string()
            }
            Ok(ThumbnailOutcome::NotImage) => {
                self.log(format!(
                    "Thumbnail skipped for {}: not an image",
                    path.display()
                ));
                String::new()
            }
            Err(error) => {
                self.log(format!("Thumbnail failed for {}: {error}", path.display()));
                String::new()
            }
        };
        Ok(Response::new(ThumbnailResponse {
            path: path.display().to_string(),
            thumbnail_path,
        }))
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
        Ok(metadata) if metadata.is_dir() => Payload::Directory(Directory {
            entries: Vec::new(),
        }),
        Ok(metadata) if metadata.is_file() => file_payload(&path, metadata.len()),
        Ok(_) => error_payload("Unsupported filesystem entry"),
        Err(error) => error_payload(error.to_string()),
    };

    BrowseResponse {
        path: display_path,
        payload: Some(payload),
    }
}

fn directory_entries(path: &Path) -> Result<Vec<FileEntry>, String> {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => return Err(error.to_string()),
    };

    let mut files = entries
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            FileEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_directory: path.is_dir(),
                path: path.display().to_string(),
                thumbnail_path: String::new(),
            }
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        right
            .is_directory
            .cmp(&left.is_directory)
            .then_with(|| left.name.starts_with('.').cmp(&right.name.starts_with('.')))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    Ok(files)
}

fn thumbnail_for(path: &Path, directory: &Path) -> Result<ThumbnailOutcome, String> {
    if !path.is_file() {
        return Ok(ThumbnailOutcome::NotImage);
    }
    let thumbnail_path = directory.join(thumbnail_filename(path));
    if thumbnail_path.is_file() {
        return Ok(ThumbnailOutcome::Cached(thumbnail_path));
    }

    let reader = match ImageReader::open(path).and_then(|reader| reader.with_guessed_format()) {
        Ok(reader) => reader,
        Err(_) => return Ok(ThumbnailOutcome::NotImage),
    };
    let image = match reader.decode() {
        Ok(image) => image,
        Err(_) => return Ok(ThumbnailOutcome::NotImage),
    };
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("could not create {}: {error}", directory.display()))?;
    let temporary_path = thumbnail_path.with_extension("tmp");
    let file = File::create(&temporary_path)
        .map_err(|error| format!("could not create {}: {error}", temporary_path.display()))?;
    image
        .thumbnail(256, 256)
        .write_to(&mut BufWriter::new(file), ImageFormat::Png)
        .map_err(|error| format!("could not write {}: {error}", temporary_path.display()))?;
    std::fs::rename(&temporary_path, &thumbnail_path).map_err(|error| {
        format!(
            "could not move {} to {}: {error}",
            temporary_path.display(),
            thumbnail_path.display()
        )
    })?;
    Ok(ThumbnailOutcome::Generated(thumbnail_path))
}

fn thumbnail_filename(path: &Path) -> String {
    let mut hasher = Sha1::new();
    hasher.update(path.as_os_str().as_encoded_bytes());
    format!("{:x}.png", hasher.finalize())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_filename_is_the_sha1_of_the_full_path() {
        assert_eq!(
            thumbnail_filename(Path::new("/tmp/image.png")),
            "0fef0cc8ed6b0e98686a7ae869b2eda3aafce32e.png"
        );
    }
}
