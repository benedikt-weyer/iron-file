use std::{
    fs::{File, OpenOptions},
    io::{self},
    path::Path,
};

mod apps;
mod browse;
mod fileops;
mod service;
mod thumbnails;

use service::FileBrowserService;

use fs2::FileExt;
use iron_file_common::{backend_lock_path, proto, socket_path};
use tokio::{
    net::{UnixListener, UnixStream},
    sync::broadcast,
};
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::Server;

use proto::file_browser_server::FileBrowserServer;

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
