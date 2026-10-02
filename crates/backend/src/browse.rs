use image::ImageReader;
use iron_file_common::proto;
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use proto::{
    BrowseResponse, BrowserError, Directory, EntryInfoField, EntryInfoResponse, FileContent,
    FileEntry, browse_response::Payload,
};

pub(crate) const MAX_PREVIEW_BYTES: u64 = 1_000_000;

pub(crate) fn browse(path: PathBuf) -> BrowseResponse {
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

pub(crate) fn entry_info(path: &Path) -> Result<EntryInfoResponse, String> {
    use lofty::{
        file::{AudioFile, TaggedFileExt},
        tag::Accessor,
    };

    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string());
    let mut fields = vec![
        info_field("Location", path.display()),
        info_field("Type", if metadata.is_dir() { "Folder" } else { "File" }),
    ];

    if metadata.is_file() {
        fields.push(info_field("Size", format_file_size(metadata.len())));
        if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
            fields.push(info_field("Extension", extension.to_ascii_uppercase()));
        }
    } else if metadata.is_dir()
        && let Ok(entries) = std::fs::read_dir(path)
    {
        fields.push(info_field("Items", entries.count()));
    }

    if let Ok(modified) = metadata.modified() {
        fields.push(info_field("Modified", format_time_ago(modified)));
    }

    if metadata.is_file() {
        if let Ok(reader) = ImageReader::open(path)
            && let Ok((width, height)) = reader.into_dimensions()
        {
            fields.push(info_field("Dimensions", format!("{width} x {height} px")));
        }

        if let Ok(audio) = lofty::read_from_path(path) {
            let properties = audio.properties();
            if !properties.duration().is_zero() {
                fields.push(info_field(
                    "Duration",
                    format_duration(properties.duration()),
                ));
            }
            if let Some(value) = properties.audio_bitrate().or(properties.overall_bitrate()) {
                fields.push(info_field("Bitrate", format!("{value} kbps")));
            }
            if let Some(value) = properties.sample_rate() {
                fields.push(info_field("Sample rate", format!("{value} Hz")));
            }
            if let Some(value) = properties.channels() {
                fields.push(info_field("Channels", value));
            }
            if let Some(value) = properties.bit_depth() {
                fields.push(info_field("Bit depth", format!("{value}-bit")));
            }
            if let Some(tag) = audio.primary_tag().or_else(|| audio.first_tag()) {
                if let Some(value) = tag.title() {
                    fields.push(info_field("Title", value));
                }
                if let Some(value) = tag.artist() {
                    fields.push(info_field("Artist", value));
                }
                if let Some(value) = tag.album() {
                    fields.push(info_field("Album", value));
                }
            }
        }
    }

    Ok(EntryInfoResponse { name, fields })
}

pub(crate) fn info_field(label: impl Into<String>, value: impl ToString) -> EntryInfoField {
    EntryInfoField {
        label: label.into(),
        value: value.to_string(),
    }
}

pub(crate) fn format_file_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub(crate) fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

pub(crate) fn format_time_ago(time: SystemTime) -> String {
    let Ok(elapsed) = SystemTime::now().duration_since(time) else {
        return "In the future".into();
    };
    match elapsed.as_secs() {
        0..=59 => "Just now".into(),
        seconds @ 60..=3_599 => format!("{} minutes ago", seconds / 60),
        seconds @ 3_600..=86_399 => format!("{} hours ago", seconds / 3_600),
        seconds => format!("{} days ago", seconds / 86_400),
    }
}

pub(crate) fn directory_entries(path: &Path) -> Result<Vec<FileEntry>, String> {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => return Err(error.to_string()),
    };

    let mut files = entries
        .filter_map(Result::ok)
        .map(|entry| {
            let started_at = std::time::Instant::now();
            let path = entry.path();
            let path_ms = started_at.elapsed().as_micros() as u64;
            let symlink_started_at = std::time::Instant::now();
            let is_symlink = std::fs::symlink_metadata(&path)
                .map(|metadata| metadata.file_type().is_symlink())
                .unwrap_or(false);
            let symlink_ms = symlink_started_at.elapsed().as_micros() as u64;
            let metadata_started_at = std::time::Instant::now();
            let metadata = std::fs::metadata(&path).ok();
            let metadata_ms = metadata_started_at.elapsed().as_micros() as u64;
            let timestamps_started_at = std::time::Instant::now();
            FileEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_directory: path.is_dir(),
                path: path.display().to_string(),
                thumbnail_path: String::new(),
                is_symlink,
                modified_at: metadata
                    .as_ref()
                    .and_then(|metadata| metadata.modified().ok())
                    .and_then(timestamp_seconds)
                    .unwrap_or_default(),
                created_at: metadata
                    .as_ref()
                    .and_then(|metadata| metadata.created().ok())
                    .and_then(timestamp_seconds)
                    .unwrap_or_default(),
                directory_complete: false,
                directory_enumeration_ms: 0,
                directory_entry_count: 0,
                entry_path_ms: path_ms,
                entry_symlink_ms: symlink_ms,
                entry_metadata_ms: metadata_ms,
                entry_timestamps_ms: timestamps_started_at.elapsed().as_micros() as u64,
            }
        })
        .collect::<Vec<_>>();
    sort_file_entries(&mut files);

    Ok(files)
}

pub(crate) fn search_directory(
    path: &Path,
    query: &str,
    max_depth: u32,
) -> Result<Vec<FileEntry>, String> {
    let query = query.to_lowercase();
    let mut entries = Vec::new();
    collect_search_entries(path, &query, max_depth, &mut entries)?;
    sort_file_entries(&mut entries);
    Ok(entries)
}

pub(crate) fn collect_search_entries(
    path: &Path,
    query: &str,
    remaining_depth: u32,
    results: &mut Vec<FileEntry>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(path)
        .map_err(|error| error.to_string())?
        .flatten()
    {
        let entry_path = entry.path();
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_name.to_lowercase().contains(query) {
            results.push(file_entry(
                entry_path.clone(),
                file_name,
                file_type.is_dir(),
            ));
        }
        if file_type.is_dir() && remaining_depth > 0 {
            collect_search_entries(
                &entry_path,
                query,
                remaining_depth.saturating_sub(1),
                results,
            )?;
        }
    }
    Ok(())
}

pub(crate) fn file_entry(path: PathBuf, name: String, is_directory: bool) -> FileEntry {
    let is_symlink = std::fs::symlink_metadata(&path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false);
    let metadata = std::fs::metadata(&path).ok();
    FileEntry {
        name,
        is_directory,
        path: path.display().to_string(),
        thumbnail_path: String::new(),
        is_symlink,
        modified_at: metadata
            .as_ref()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(timestamp_seconds)
            .unwrap_or_default(),
        created_at: metadata
            .as_ref()
            .and_then(|metadata| metadata.created().ok())
            .and_then(timestamp_seconds)
            .unwrap_or_default(),
        ..FileEntry::default()
    }
}

pub(crate) fn timestamp_seconds(time: std::time::SystemTime) -> Option<i64> {
    time.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
}

pub(crate) fn sort_file_entries(entries: &mut [FileEntry]) {
    entries.sort_by_key(|entry| {
        (
            !entry.is_directory,
            entry.name.starts_with('.'),
            entry.name.to_lowercase(),
        )
    });
}

pub(crate) fn file_payload(path: &Path, size: u64) -> Payload {
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

pub(crate) fn error_payload(message: impl Into<String>) -> Payload {
    Payload::Error(BrowserError {
        message: message.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_entries_sort_last_within_their_category() {
        let mut entries = vec![
            FileEntry {
                name: ".hidden-file".into(),
                is_directory: false,
                path: String::new(),
                thumbnail_path: String::new(),
                is_symlink: false,
                modified_at: 0,
                created_at: 0,
                ..FileEntry::default()
            },
            FileEntry {
                name: "visible-file".into(),
                is_directory: false,
                path: String::new(),
                thumbnail_path: String::new(),
                is_symlink: false,
                modified_at: 0,
                created_at: 0,
                ..FileEntry::default()
            },
            FileEntry {
                name: ".hidden-folder".into(),
                is_directory: true,
                path: String::new(),
                thumbnail_path: String::new(),
                is_symlink: false,
                modified_at: 0,
                created_at: 0,
                ..FileEntry::default()
            },
            FileEntry {
                name: "visible-folder".into(),
                is_directory: true,
                path: String::new(),
                thumbnail_path: String::new(),
                is_symlink: false,
                modified_at: 0,
                created_at: 0,
                ..FileEntry::default()
            },
        ];

        sort_file_entries(&mut entries);

        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            [
                "visible-folder",
                ".hidden-folder",
                "visible-file",
                ".hidden-file"
            ]
        );
    }
}
