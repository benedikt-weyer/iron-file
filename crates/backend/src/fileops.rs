use std::{
    fs::{File, OpenOptions},
    io::{self},
    path::{Path, PathBuf},
};
use tonic::Status;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::FileOptions};

pub(crate) fn compress_entries(
    sources: Vec<PathBuf>,
    destination: &Path,
    compression_level: i32,
    compression_type: CompressionMethod,
) -> Result<PathBuf, String> {
    if sources.is_empty() || !destination.is_dir() {
        return Err("select entries and a destination directory".into());
    }
    let name = if sources.len() == 1 {
        sources[0]
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("{name}.zip"))
            .unwrap_or_else(|| "archive.zip".into())
    } else {
        "archive.zip".into()
    };
    let archive_path = available_copy_path(destination.join(name));
    let file = File::create(&archive_path)
        .map_err(|error| format!("could not create {}: {error}", archive_path.display()))?;
    let mut archive = ZipWriter::new(file);
    let options = FileOptions::default()
        .compression_method(compression_type)
        .compression_level(Some(compression_level));
    for source in &sources {
        let root = source
            .file_name()
            .ok_or_else(|| format!("{} has no file name", source.display()))?;
        add_archive_path(&mut archive, source, Path::new(root), options)?;
    }
    archive
        .finish()
        .map_err(|error| format!("could not finish archive: {error}"))?;
    Ok(archive_path)
}

pub(crate) fn compression_method(value: &str) -> Result<CompressionMethod, Status> {
    match value {
        "store" => Ok(CompressionMethod::Stored),
        "deflate" | "" => Ok(CompressionMethod::Deflated),
        "bzip2" => Ok(CompressionMethod::Bzip2),
        "zstd" => Ok(CompressionMethod::Zstd),
        _ => Err(Status::invalid_argument("unsupported compression type")),
    }
}

pub(crate) fn add_archive_path(
    archive: &mut ZipWriter<File>,
    source: &Path,
    archive_path: &Path,
    options: FileOptions,
) -> Result<(), String> {
    let name = archive_path.to_string_lossy().replace('\\', "/");
    if source.is_dir() {
        archive
            .add_directory(format!("{name}/"), options)
            .map_err(|error| error.to_string())?;
        for entry in std::fs::read_dir(source).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            add_archive_path(
                archive,
                &entry.path(),
                &archive_path.join(entry.file_name()),
                options,
            )?;
        }
    } else if source.is_file() {
        archive
            .start_file(name, options)
            .map_err(|error| error.to_string())?;
        let mut input = File::open(source).map_err(|error| error.to_string())?;
        io::copy(&mut input, archive).map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub(crate) fn extract_archives(
    sources: Vec<PathBuf>,
    destination: &Path,
) -> Result<Vec<PathBuf>, String> {
    if !destination.is_dir() {
        return Err(format!("{} is not a directory", destination.display()));
    }
    sources
        .into_iter()
        .map(|source| {
            if source
                .extension()
                .is_none_or(|extension| extension != "zip")
            {
                return Err(format!("{} is not a ZIP archive", source.display()));
            }
            let name = source.file_stem().unwrap_or_default();
            let output = available_copy_path(destination.join(name));
            std::fs::create_dir(&output).map_err(|error| error.to_string())?;
            let file = File::open(&source).map_err(|error| error.to_string())?;
            let mut archive = ZipArchive::new(file).map_err(|error| error.to_string())?;
            for index in 0..archive.len() {
                let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
                let Some(name) = entry.enclosed_name().map(PathBuf::from) else {
                    return Err(format!("{} contains an unsafe path", source.display()));
                };
                let path = output.join(name);
                if entry.is_dir() {
                    std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
                } else {
                    let parent = path
                        .parent()
                        .ok_or_else(|| "archive entry has no parent".to_string())?;
                    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                    let mut output_file = File::create(path).map_err(|error| error.to_string())?;
                    io::copy(&mut entry, &mut output_file).map_err(|error| error.to_string())?;
                }
            }
            Ok(output)
        })
        .collect()
}

pub(crate) fn copy_entries(
    sources: impl IntoIterator<Item = PathBuf>,
    destination: &Path,
) -> Result<Vec<PathBuf>, String> {
    if !destination.is_dir() {
        return Err(format!("{} is not a directory", destination.display()));
    }

    sources
        .into_iter()
        .map(|source| {
            if !source.exists() {
                return Err(format!("{} does not exist", source.display()));
            }
            if source.is_dir() && destination.starts_with(&source) {
                return Err(format!("cannot copy {} into itself", source.display()));
            }
            let name = source
                .file_name()
                .ok_or_else(|| format!("{} has no file name", source.display()))?;
            let target = available_copy_path(destination.join(name));
            copy_path(&source, &target)?;
            Ok(target)
        })
        .collect()
}

pub(crate) fn delete_entries(paths: &[PathBuf]) -> Result<(), String> {
    for path in paths {
        let metadata = std::fs::symlink_metadata(path)
            .map_err(|error| format!("could not inspect {}: {error}", path.display()))?;
        let result = if metadata.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        };
        result.map_err(|error| format!("could not delete {}: {error}", path.display()))?;
    }
    Ok(())
}

pub(crate) fn create_symlinks(
    sources: &[PathBuf],
    destination: &Path,
) -> Result<Vec<PathBuf>, String> {
    if !destination.is_dir() {
        return Err(format!("{} is not a directory", destination.display()));
    }
    sources
        .iter()
        .map(|source| {
            if !source.exists() {
                return Err(format!("{} does not exist", source.display()));
            }
            let name = source
                .file_name()
                .ok_or_else(|| format!("{} has no file name", source.display()))?;
            let target = available_copy_path(destination.join(name));
            create_symlink(source, &target)?;
            Ok(target)
        })
        .collect()
}

pub(crate) fn create_entry(
    parent: &Path,
    name: &str,
    is_directory: bool,
) -> Result<PathBuf, String> {
    if !parent.is_dir() {
        return Err(format!("{} is not a directory", parent.display()));
    }
    let name_path = single_file_name(name)?;
    let path = parent.join(name_path);
    if is_directory {
        std::fs::create_dir(&path)
    } else {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map(|_| ())
    }
    .map_err(|error| format!("could not create {}: {error}", path.display()))?;
    Ok(path)
}

pub(crate) fn rename_entry(path: &Path, name: &str) -> Result<PathBuf, String> {
    std::fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect {}: {error}", path.display()))?;
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    let target = parent.join(single_file_name(name)?);
    if target == path {
        return Ok(target);
    }
    match std::fs::symlink_metadata(&target) {
        Ok(_) => return Err(format!("{} already exists", target.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!("could not inspect {}: {error}", target.display()));
        }
    }
    std::fs::rename(path, &target)
        .map_err(|error| format!("could not rename {}: {error}", path.display()))?;
    Ok(target)
}

pub(crate) fn single_file_name(name: &str) -> Result<&Path, String> {
    let name_path = Path::new(name);
    if name.is_empty()
        || name == "."
        || name == ".."
        || name_path.is_absolute()
        || name_path.components().count() != 1
    {
        Err("name must be a single non-empty file name".into())
    } else {
        Ok(name_path)
    }
}

pub(crate) fn available_copy_path(candidate: PathBuf) -> PathBuf {
    if !candidate.exists() {
        return candidate;
    }
    let parent = candidate.parent().unwrap_or_else(|| Path::new("."));
    let name = candidate
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("copy");
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
        _ => (name, String::new()),
    };
    for number in 1.. {
        let path = parent.join(format!("{stem} (copy {number}){extension}"));
        if !path.exists() {
            return path;
        }
    }
    unreachable!("unbounded copy name search")
}

pub(crate) fn copy_path(source: &Path, target: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(source).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return copy_symlink(source, target);
    }
    if metadata.is_file() {
        std::fs::copy(source, target)
            .map(|_| ())
            .map_err(|error| format!("could not copy {}: {error}", source.display()))
    } else if metadata.is_dir() {
        std::fs::create_dir(target)
            .map_err(|error| format!("could not create {}: {error}", target.display()))?;
        for entry in std::fs::read_dir(source).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            copy_path(&entry.path(), &target.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        Err(format!(
            "unsupported filesystem entry: {}",
            source.display()
        ))
    }
}

#[cfg(unix)]
pub(crate) fn copy_symlink(source: &Path, target: &Path) -> Result<(), String> {
    let link_target = std::fs::read_link(source)
        .map_err(|error| format!("could not read symbolic link {}: {error}", source.display()))?;
    std::os::unix::fs::symlink(link_target, target)
        .map_err(|error| format!("could not copy symbolic link {}: {error}", source.display()))
}

#[cfg(unix)]
pub(crate) fn create_symlink(source: &Path, target: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(source, target).map_err(|error| {
        format!(
            "could not create symbolic link {}: {error}",
            target.display()
        )
    })
}

#[cfg(not(unix))]
pub(crate) fn copy_symlink(source: &Path, _: &Path) -> Result<(), String> {
    Err(format!(
        "copying symbolic links is not supported on this platform: {}",
        source.display()
    ))
}

#[cfg(not(unix))]
pub(crate) fn create_symlink(source: &Path, _: &Path) -> Result<(), String> {
    Err(format!(
        "creating symbolic links is not supported on this platform: {}",
        source.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_entries_copies_files_directories_and_avoids_collisions() {
        let root = std::env::temp_dir().join(format!(
            "iron-file-copy-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::create_dir_all(source.join("folder")).unwrap();
        std::fs::create_dir_all(&destination).unwrap();
        std::fs::write(source.join("note.txt"), "note").unwrap();
        std::fs::write(source.join("folder/nested.txt"), "nested").unwrap();

        let copied = copy_entries(
            vec![source.join("note.txt"), source.join("folder")],
            &destination,
        )
        .unwrap();
        assert_eq!(copied.len(), 2);
        assert_eq!(
            std::fs::read_to_string(destination.join("note.txt")).unwrap(),
            "note"
        );
        assert_eq!(
            std::fs::read_to_string(destination.join("folder/nested.txt")).unwrap(),
            "nested"
        );

        copy_entries(vec![source.join("note.txt")], &destination).unwrap();
        assert_eq!(
            std::fs::read_to_string(destination.join("note (copy 1).txt")).unwrap(),
            "note"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn delete_entries_removes_files_and_directories_permanently() {
        let root = std::env::temp_dir().join(format!(
            "iron-file-delete-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let file = root.join("file.txt");
        let directory = root.join("directory");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(&file, "file").unwrap();
        std::fs::write(directory.join("nested.txt"), "nested").unwrap();

        delete_entries(&[file.clone(), directory.clone()]).unwrap();

        assert!(!file.exists());
        assert!(!directory.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn create_symlinks_creates_links_to_the_source_entries() {
        let root = std::env::temp_dir().join(format!(
            "iron-file-symlink-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let source = root.join("source.txt");
        let destination = root.join("destination");
        std::fs::create_dir_all(&destination).unwrap();
        std::fs::write(&source, "source").unwrap();

        let created = create_symlinks(std::slice::from_ref(&source), &destination).unwrap();

        assert_eq!(created, vec![destination.join("source.txt")]);
        assert_eq!(std::fs::read_to_string(&created[0]).unwrap(), "source");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn create_entry_creates_files_and_folders_with_safe_names() {
        let root = std::env::temp_dir().join(format!(
            "iron-file-create-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(&root).unwrap();

        let file = create_entry(&root, "new.txt", false).unwrap();
        let directory = create_entry(&root, "new-folder", true).unwrap();

        assert!(file.is_file());
        assert!(directory.is_dir());
        assert!(create_entry(&root, "nested/name", false).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rename_entry_renames_files_and_folders_without_overwriting() {
        let root = std::env::temp_dir().join(format!(
            "iron-file-rename-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(root.join("folder")).unwrap();
        let file = root.join("file.txt");
        std::fs::write(&file, "contents").unwrap();

        let renamed_file = rename_entry(&file, "renamed.txt").unwrap();
        let renamed_folder = rename_entry(&root.join("folder"), "renamed-folder").unwrap();

        assert_eq!(std::fs::read_to_string(renamed_file).unwrap(), "contents");
        assert!(renamed_folder.is_dir());
        assert!(rename_entry(&root.join("renamed.txt"), "nested/name").is_err());
        assert!(rename_entry(&root.join("renamed.txt"), "renamed-folder").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn compresses_and_extracts_selected_entries() {
        let root = std::env::temp_dir().join(format!(
            "iron-file-archive-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let source = root.join("source");
        let archives = root.join("archives");
        let extracted = root.join("extracted");
        std::fs::create_dir_all(source.join("folder")).unwrap();
        std::fs::create_dir_all(&archives).unwrap();
        std::fs::create_dir_all(&extracted).unwrap();
        std::fs::write(source.join("folder/file.txt"), "contents").unwrap();

        let archive = compress_entries(
            vec![source.join("folder")],
            &archives,
            6,
            CompressionMethod::Deflated,
        )
        .unwrap();
        let outputs = extract_archives(vec![archive], &extracted).unwrap();

        assert_eq!(outputs.len(), 1);
        assert_eq!(
            std::fs::read_to_string(outputs[0].join("folder/file.txt")).unwrap(),
            "contents"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
