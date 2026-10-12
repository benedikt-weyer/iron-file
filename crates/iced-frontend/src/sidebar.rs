use super::*;

impl Gui {
    pub(super) fn active_sidebar_locations(&self) -> Vec<SidebarLocation> {
        self.active_profile
            .as_deref()
            .and_then(|path| self.profiles.iter().find(|profile| profile.path == path))
            .map(|profile| profile.sidebar_locations.clone())
            .unwrap_or_default()
    }

    pub(super) fn add_context_folder_to_sidebar(&mut self) {
        let Some(ContextEntry {
            path,
            is_directory: true,
            ..
        }) = self.context_entry.take()
        else {
            return;
        };
        let mut locations = self.active_sidebar_locations();
        if locations.iter().any(|location| location.path == path) {
            return;
        }
        let label = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| path.display().to_string());
        locations.push(SidebarLocation {
            label,
            path,
            icon: None,
        });
        self.save_sidebar_locations(locations);
    }

    pub(super) fn set_sidebar_location_icon(&mut self, path: PathBuf, icon: Option<String>) {
        let mut locations = self.active_sidebar_locations();
        let Some(location) = locations.iter_mut().find(|location| location.path == path) else {
            return;
        };
        location.icon = icon;
        self.context_entry = None;
        self.save_sidebar_locations(locations);
    }

    pub(super) fn remove_context_folder_from_sidebar(&mut self) {
        let Some(ContextEntry {
            path,
            is_directory: true,
            ..
        }) = self.context_entry.take()
        else {
            return;
        };
        let mut locations = self.active_sidebar_locations();
        locations.retain(|location| location.path != path);
        self.save_sidebar_locations(locations);
    }

    pub(super) fn drag_in_progress(&self) -> bool {
        self.dragging_entries.is_some() || self.external_drag_over
    }

    /// Starts a Wayland drag when an in-window drag leaves the window, so the files can be
    /// dropped into another window or application.
    pub(super) fn start_external_drag_if_outside(&mut self) {
        let position = self.window_cursor_position;
        let outside = position.x < 0.0
            || position.y < 0.0
            || position.x > self.window_size.width
            || position.y > self.window_size.height;
        if !outside
            || self.external_drag_started
            || self.picker.is_some()
            || winit::platform::wayland::file_drag_active()
        {
            return;
        }
        let Some(mut entries) = self.dragging_entries.clone() else {
            return;
        };
        entries.sort();
        if winit::platform::wayland::start_file_drag(entries) {
            self.external_drag_started = true;
            self.entry_drop_target = None;
            self.sidebar_drop_target = None;
        }
    }

    /// Handles files dropped from another window or application onto the folder, sidebar
    /// location or current directory under the pointer.
    pub(super) fn drop_external_files(&mut self) -> Task<Message> {
        let sources = std::mem::take(&mut self.pending_dropped_files);
        let destination = self
            .entry_drop_target
            .take()
            .or_else(|| self.sidebar_drop_target.take())
            .unwrap_or_else(|| self.directory_path.clone());
        self.external_drag_over = false;
        let is_move = winit::platform::wayland::last_file_drop_is_move();
        let sources = sources
            .into_iter()
            .filter(|path| {
                !is_move || (path != &destination && path.parent() != Some(destination.as_path()))
            })
            .collect::<Vec<_>>();
        if sources.is_empty() {
            self.status = "Item(s) are already in this folder".into();
            return Task::none();
        }
        if is_move {
            let to_delete = sources.clone();
            self.status = format!("Moving {} item(s)...", to_delete.len());
            Task::perform(copy_entries(sources, destination), move |result| {
                Message::CutPasteFinished {
                    sources: to_delete.clone(),
                    result,
                }
            })
        } else {
            self.status = format!("Copying {} item(s)...", sources.len());
            Task::perform(
                copy_entries(sources, destination),
                Message::FileCopyFinished,
            )
        }
    }

    pub(super) fn drop_dragged_entries(&mut self, target: PathBuf) -> Task<Message> {
        self.entry_drop_target = None;
        let Some(sources) = self.dragging_entries.take() else {
            return Task::none();
        };
        let sources = sources
            .into_iter()
            .filter(|path| path != &target && path.parent() != Some(target.as_path()))
            .collect::<Vec<_>>();
        if sources.is_empty() {
            return Task::none();
        }
        let to_delete = sources.clone();
        self.status = format!("Moving {} item(s)...", to_delete.len());
        Task::perform(copy_entries(sources, target), move |result| {
            Message::CutPasteFinished {
                sources: to_delete.clone(),
                result,
            }
        })
    }

    pub(super) fn release_sidebar_location(&mut self, target: PathBuf) -> Task<Message> {
        let Some(source) = self.dragging_sidebar_location.take() else {
            return Task::none();
        };
        self.sidebar_drop_target = None;
        self.sidebar_drop_at_end = false;
        if source == target {
            return self.open_path(target);
        }
        let mut locations = self.active_sidebar_locations();
        let Some(source_index) = locations
            .iter()
            .position(|location| location.path == source)
        else {
            return Task::none();
        };
        let location = locations.remove(source_index);
        let Some(target_index) = locations
            .iter()
            .position(|location| location.path == target)
        else {
            return Task::none();
        };
        locations.insert(target_index, location);
        self.save_sidebar_locations(locations);
        Task::none()
    }

    pub(super) fn release_sidebar_location_at_end(&mut self) -> Task<Message> {
        let Some(source) = self.dragging_sidebar_location.take() else {
            return Task::none();
        };
        self.sidebar_drop_target = None;
        self.sidebar_drop_at_end = false;
        let mut locations = self.active_sidebar_locations();
        let Some(source_index) = locations
            .iter()
            .position(|location| location.path == source)
        else {
            return Task::none();
        };
        let location = locations.remove(source_index);
        locations.push(location);
        self.save_sidebar_locations(locations);
        Task::none()
    }
}
