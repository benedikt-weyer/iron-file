use std::path::Path;

use gio::prelude::*;

use crate::proto::AppChoice;

fn content_type(path: &Path) -> Result<String, String> {
    let info = gio::File::for_path(path)
        .query_info(
            gio::FILE_ATTRIBUTE_STANDARD_CONTENT_TYPE,
            gio::FileQueryInfoFlags::NONE,
            gio::Cancellable::NONE,
        )
        .map_err(|error| {
            format!(
                "Could not determine the type of {}: {error}",
                path.display()
            )
        })?;
    info.content_type()
        .map(|mime| mime.to_string())
        .ok_or_else(|| format!("No MIME type was returned for {}", path.display()))
}

fn apps_for_content_type(mime: &str) -> Vec<gio::AppInfo> {
    gio::AppInfo::all_for_type(mime)
        .into_iter()
        .filter(|app| app.id().is_some())
        .collect()
}

fn find_app(mime: &str, app_id: &str) -> Result<gio::AppInfo, String> {
    apps_for_content_type(mime)
        .into_iter()
        .find(|app| app.id().is_some_and(|id| id.as_str() == app_id))
        .ok_or_else(|| format!("{app_id} is no longer available"))
}

fn launch(app: &gio::AppInfo, path: &Path) -> Result<(), String> {
    app.launch(&[gio::File::for_path(path)], gio::AppLaunchContext::NONE)
        .map_err(|error| {
            format!(
                "Could not open {} with {}: {error}",
                path.display(),
                app.name()
            )
        })
}

pub fn default_app(path: &Path) -> Result<AppChoice, String> {
    let mime = content_type(path)?;
    let app = gio::AppInfo::default_for_type(&mime, false)
        .ok_or_else(|| format!("No default application is configured for {mime}"))?;
    Ok(AppChoice {
        id: app.id().map(|id| id.to_string()).unwrap_or_default(),
        name: app.name().to_string(),
        is_default: true,
    })
}

pub fn list_apps(path: &Path) -> Result<Vec<AppChoice>, String> {
    let mime = content_type(path)?;
    let default_id = gio::AppInfo::default_for_type(&mime, false).and_then(|app| app.id());
    let mut choices: Vec<AppChoice> = apps_for_content_type(&mime)
        .into_iter()
        .map(|app| {
            let id = app.id().expect("filtered for Some id").to_string();
            let is_default = default_id
                .as_ref()
                .is_some_and(|default_id| default_id.as_str() == id);
            AppChoice {
                id,
                name: app.name().to_string(),
                is_default,
            }
        })
        .collect();
    choices.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    choices.dedup_by(|left, right| left.id == right.id);
    Ok(choices)
}

pub fn open_default(path: &Path) -> Result<(), String> {
    let mime = content_type(path)?;
    let app = gio::AppInfo::default_for_type(&mime, false)
        .ok_or_else(|| format!("No default application is configured for {mime}"))?;
    launch(&app, path)
}

pub fn open_with(path: &Path, app_id: &str) -> Result<(), String> {
    let app = find_app(&content_type(path)?, app_id)?;
    launch(&app, path)
}

pub fn set_default(path: &Path, app_id: &str) -> Result<(), String> {
    let mime = content_type(path)?;
    let app = find_app(&mime, app_id)?;
    app.set_as_default_for_type(&mime).map_err(|error| {
        format!(
            "Could not set {} as the default for {mime}: {error}",
            app.name()
        )
    })
}
