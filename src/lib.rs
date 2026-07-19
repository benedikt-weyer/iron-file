use std::{
    fs,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
};

/// Events sent from a GUI implementation to the shared background worker.
#[derive(Debug)]
pub enum GuiEvent {
    OpenPath(PathBuf),
}

/// Updates sent from the shared background worker to a GUI implementation.
#[derive(Debug)]
pub enum BackendEvent {
    Directory {
        path: PathBuf,
        entries: Vec<FileEntry>,
    },
    FileContent {
        path: PathBuf,
        content: String,
    },
    Error {
        path: PathBuf,
        message: String,
    },
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_directory: bool,
}

/// Starts the backend used by every GUI implementation.
pub fn start_backend() -> (Sender<GuiEvent>, Receiver<BackendEvent>, JoinHandle<()>) {
    let (gui_sender, gui_receiver) = mpsc::channel::<GuiEvent>();
    let (backend_sender, backend_receiver) = mpsc::channel::<BackendEvent>();

    let worker = thread::spawn(move || run_background_worker(gui_receiver, backend_sender));

    (gui_sender, backend_receiver, worker)
}

fn run_background_worker(gui_receiver: Receiver<GuiEvent>, backend_sender: Sender<BackendEvent>) {
    while let Ok(GuiEvent::OpenPath(path)) = gui_receiver.recv() {
        if backend_sender.send(open_path(path)).is_err() {
            break;
        }
    }
}

fn open_path(path: PathBuf) -> BackendEvent {
    match fs::metadata(&path) {
        Ok(metadata) if metadata.is_dir() => read_directory(path),
        Ok(metadata) if metadata.is_file() => read_file(path, metadata.len()),
        Ok(_) => BackendEvent::Error {
            path,
            message: "Unsupported filesystem entry".into(),
        },
        Err(error) => BackendEvent::Error {
            path,
            message: error.to_string(),
        },
    }
}

fn read_directory(path: PathBuf) -> BackendEvent {
    let entries = match fs::read_dir(&path) {
        Ok(entries) => entries,
        Err(error) => {
            return BackendEvent::Error {
                path,
                message: error.to_string(),
            };
        }
    };

    let mut files = entries
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            FileEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_directory: path.is_dir(),
                path,
            }
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        right
            .is_directory
            .cmp(&left.is_directory)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    BackendEvent::Directory {
        path,
        entries: files,
    }
}

fn read_file(path: PathBuf, size: u64) -> BackendEvent {
    const MAX_PREVIEW_BYTES: u64 = 1_000_000;

    if size > MAX_PREVIEW_BYTES {
        return BackendEvent::FileContent {
            path,
            content: format!("Preview unavailable: file is larger than {MAX_PREVIEW_BYTES} bytes."),
        };
    }

    match fs::read(&path) {
        Ok(contents) => match String::from_utf8(contents) {
            Ok(content) => BackendEvent::FileContent { path, content },
            Err(_) => BackendEvent::FileContent {
                path,
                content: "Preview unavailable: binary file.".into(),
            },
        },
        Err(error) => BackendEvent::Error {
            path,
            message: error.to_string(),
        },
    }
}
