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
