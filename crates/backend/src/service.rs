use crate::{
    apps,
    browse::{browse, directory_entries, entry_info, search_directory},
    fileops::{
        compress_entries, compression_method, copy_entries, create_entry, create_symlinks,
        delete_entries, extract_archives, rename_entry,
    },
    thumbnails::{ThumbnailOutcome, thumbnail_for},
};
use iron_file_common::proto;
use std::{
    path::{Path, PathBuf},
    pin::Pin,
};
use tokio::sync::{broadcast, mpsc};
use tokio_stream::{Stream, wrappers::ReceiverStream};
use tonic::{Request, Response, Status};

use proto::{
    AppChoice, BrowseResponse, CreateEntryRequest, DeleteEntriesRequest, EntryInfoRequest,
    EntryInfoResponse, FileCommandRequest, FileCommandResponse, FileEntry, ListDirectoryRequest,
    ListOpenWithAppsResponse, LogEntry, LogStreamRequest, OpenPathRequest, OpenWithRequest,
    OpenWithResponse, RenameEntryRequest, SearchDirectoryRequest, SearchDirectoryResponse,
    ThumbnailRequest, ThumbnailResponse, browse_response::Payload,
    file_browser_server::FileBrowser,
};

pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, Status> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .map_err(Status::failed_precondition)
}

#[derive(Clone)]
pub(crate) struct FileBrowserService {
    pub(crate) logs: broadcast::Sender<String>,
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
        let started_at = std::time::Instant::now();
        let entries = directory_entries(&path).map_err(Status::internal)?;
        let entry_count = entries.len() as u32;
        let enumeration_ms = started_at.elapsed().as_millis() as u64;
        Ok(Response::new(Box::pin(tokio_stream::iter(
            entries
                .into_iter()
                .map(Ok)
                .chain(std::iter::once(Ok(FileEntry {
                    directory_complete: true,
                    directory_enumeration_ms: enumeration_ms,
                    directory_entry_count: entry_count,
                    ..FileEntry::default()
                }))),
        ))))
    }

    async fn search_directory(
        &self,
        request: Request<SearchDirectoryRequest>,
    ) -> Result<Response<SearchDirectoryResponse>, Status> {
        let request = request.into_inner();
        let path = PathBuf::from(request.path);
        self.log(format!("Searching {}", path.display()));
        search_directory(&path, &request.query, request.max_depth)
            .map(|entries| Response::new(SearchDirectoryResponse { entries }))
            .map_err(Status::internal)
    }

    async fn create_thumbnail(
        &self,
        request: Request<ThumbnailRequest>,
    ) -> Result<Response<ThumbnailResponse>, Status> {
        let request = request.into_inner();
        let path = PathBuf::from(request.path);
        let thumbnail_path = match thumbnail_for(&path, Path::new(&request.thumbnail_directory)) {
            Ok(ThumbnailOutcome::Cached(thumbnail_path)) => {
                self.log(format!("Thumbnail cache hit for {}", path.display()));
                thumbnail_path.display().to_string()
            }
            Ok(ThumbnailOutcome::Generated(thumbnail_path)) => {
                self.log(format!("Thumbnail generated for {}", path.display()));
                thumbnail_path.display().to_string()
            }
            Ok(ThumbnailOutcome::NotImage) => String::new(),
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

    async fn inspect_entry(
        &self,
        request: Request<EntryInfoRequest>,
    ) -> Result<Response<EntryInfoResponse>, Status> {
        let path = PathBuf::from(request.into_inner().path);
        self.log(format!("Inspecting {}", path.display()));
        entry_info(&path)
            .map(Response::new)
            .map_err(Status::internal)
    }

    async fn get_default_app(
        &self,
        request: Request<EntryInfoRequest>,
    ) -> Result<Response<AppChoice>, Status> {
        let path = PathBuf::from(request.into_inner().path);
        blocking(move || apps::default_app(&path))
            .await
            .map(Response::new)
    }

    async fn list_open_with_apps(
        &self,
        request: Request<EntryInfoRequest>,
    ) -> Result<Response<ListOpenWithAppsResponse>, Status> {
        let path = PathBuf::from(request.into_inner().path);
        blocking(move || apps::list_apps(&path))
            .await
            .map(|apps| Response::new(ListOpenWithAppsResponse { apps }))
    }

    async fn open_file(
        &self,
        request: Request<EntryInfoRequest>,
    ) -> Result<Response<OpenWithResponse>, Status> {
        let path = PathBuf::from(request.into_inner().path);
        self.log(format!("Opening {}", path.display()));
        blocking(move || apps::open_default(&path))
            .await
            .map(|()| Response::new(OpenWithResponse {}))
    }

    async fn open_with_app(
        &self,
        request: Request<OpenWithRequest>,
    ) -> Result<Response<OpenWithResponse>, Status> {
        let request = request.into_inner();
        let path = PathBuf::from(request.path);
        self.log(format!(
            "Opening {} with {}",
            path.display(),
            request.app_id
        ));
        blocking(move || apps::open_with(&path, &request.app_id))
            .await
            .map(|()| Response::new(OpenWithResponse {}))
    }

    async fn set_default_app(
        &self,
        request: Request<OpenWithRequest>,
    ) -> Result<Response<OpenWithResponse>, Status> {
        let request = request.into_inner();
        let path = PathBuf::from(request.path);
        self.log(format!(
            "Setting {} as default for {}",
            request.app_id,
            path.display()
        ));
        blocking(move || apps::set_default(&path, &request.app_id))
            .await
            .map(|()| Response::new(OpenWithResponse {}))
    }

    async fn copy_entries(
        &self,
        request: Request<FileCommandRequest>,
    ) -> Result<Response<FileCommandResponse>, Status> {
        let request = request.into_inner();
        let destination = PathBuf::from(request.destination);
        self.log(format!(
            "Copying {} item(s) to {}",
            request.sources.len(),
            destination.display()
        ));
        let copied_paths =
            copy_entries(request.sources.into_iter().map(PathBuf::from), &destination).map_err(
                |error| {
                    self.log(format!("Copy failed: {error}"));
                    Status::internal(error)
                },
            )?;
        self.log(format!(
            "Copied {} item(s) to {}",
            copied_paths.len(),
            destination.display()
        ));
        Ok(Response::new(FileCommandResponse {
            copied_paths: copied_paths
                .into_iter()
                .map(|path| path.display().to_string())
                .collect(),
        }))
    }

    async fn delete_entries(
        &self,
        request: Request<DeleteEntriesRequest>,
    ) -> Result<Response<FileCommandResponse>, Status> {
        let paths = request
            .into_inner()
            .paths
            .into_iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        self.log(format!("Deleting {} item(s)", paths.len()));
        delete_entries(&paths).map_err(|error| {
            self.log(format!("Delete failed: {error}"));
            Status::internal(error)
        })?;
        self.log(format!("Deleted {} item(s)", paths.len()));
        Ok(Response::new(FileCommandResponse {
            copied_paths: paths
                .into_iter()
                .map(|path| path.display().to_string())
                .collect(),
        }))
    }

    async fn create_symlinks(
        &self,
        request: Request<FileCommandRequest>,
    ) -> Result<Response<FileCommandResponse>, Status> {
        let request = request.into_inner();
        let destination = PathBuf::from(request.destination);
        let sources = request
            .sources
            .into_iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        self.log(format!(
            "Creating {} symbolic link(s) in {}",
            sources.len(),
            destination.display()
        ));
        let paths = create_symlinks(&sources, &destination).map_err(|error| {
            self.log(format!("Creating symbolic links failed: {error}"));
            Status::internal(error)
        })?;
        Ok(Response::new(FileCommandResponse {
            copied_paths: paths
                .into_iter()
                .map(|path| path.display().to_string())
                .collect(),
        }))
    }

    async fn create_entry(
        &self,
        request: Request<CreateEntryRequest>,
    ) -> Result<Response<FileCommandResponse>, Status> {
        let request = request.into_inner();
        let parent = PathBuf::from(request.parent);
        let path = create_entry(&parent, &request.name, request.is_directory).map_err(|error| {
            self.log(format!("Creating entry failed: {error}"));
            Status::invalid_argument(error)
        })?;
        self.log(format!("Created {}", path.display()));
        Ok(Response::new(FileCommandResponse {
            copied_paths: vec![path.display().to_string()],
        }))
    }

    async fn rename_entry(
        &self,
        request: Request<RenameEntryRequest>,
    ) -> Result<Response<FileCommandResponse>, Status> {
        let request = request.into_inner();
        let path = PathBuf::from(request.path);
        let renamed = rename_entry(&path, &request.name).map_err(|error| {
            self.log(format!("Renaming entry failed: {error}"));
            Status::invalid_argument(error)
        })?;
        self.log(format!(
            "Renamed {} to {}",
            path.display(),
            renamed.display()
        ));
        Ok(Response::new(FileCommandResponse {
            copied_paths: vec![renamed.display().to_string()],
        }))
    }

    async fn compress_entries(
        &self,
        request: Request<FileCommandRequest>,
    ) -> Result<Response<FileCommandResponse>, Status> {
        let request = request.into_inner();
        let destination = PathBuf::from(request.destination);
        let compression_level = request.compression_level.clamp(0, 9);
        let compression_type = compression_method(&request.compression_type)?;
        let archive = compress_entries(
            request.sources.into_iter().map(PathBuf::from).collect(),
            &destination,
            compression_level,
            compression_type,
        )
        .map_err(Status::internal)?;
        self.log(format!("Created archive {}", archive.display()));
        Ok(Response::new(FileCommandResponse {
            copied_paths: vec![archive.display().to_string()],
        }))
    }

    async fn extract_archives(
        &self,
        request: Request<FileCommandRequest>,
    ) -> Result<Response<FileCommandResponse>, Status> {
        let request = request.into_inner();
        let destination = PathBuf::from(request.destination);
        let paths = extract_archives(
            request.sources.into_iter().map(PathBuf::from).collect(),
            &destination,
        )
        .map_err(Status::internal)?;
        self.log(format!("Extracted {} archive(s)", paths.len()));
        Ok(Response::new(FileCommandResponse {
            copied_paths: paths
                .into_iter()
                .map(|path| path.display().to_string())
                .collect(),
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
    pub(crate) fn log(&self, message: impl Into<String>) {
        let message = message.into();
        eprintln!("{message}");
        let _ = self.logs.send(message);
    }
}
