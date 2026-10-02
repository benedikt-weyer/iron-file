use super::*;

impl Gui {
    pub(super) fn open_path(&mut self, path: PathBuf) -> Task<Message> {
        self.search = None;
        self.request_path(path, HistoryRequest::New)
    }

    pub(super) fn navigate_history(&mut self, direction: isize) -> Task<Message> {
        let Some(index) = self.history_index else {
            return Task::none();
        };
        let Some(target_index) = index.checked_add_signed(direction) else {
            return Task::none();
        };
        let Some(path) = self.history.get(target_index).cloned() else {
            return Task::none();
        };
        self.request_path(path, HistoryRequest::Existing(target_index))
    }

    pub(super) fn request_path(&mut self, path: PathBuf, history: HistoryRequest) -> Task<Message> {
        self.editing_address = false;
        self.status = format!("Loading {}", path.display());
        self.folder_load_started = Some(Instant::now());
        let thumbnail_directory = self.active_browser_settings().thumbnail_location;
        Task::perform(
            browse_with_thumbnails(path, Some(thumbnail_directory)),
            move |result| Message::BrowseFinished { result, history },
        )
    }

    pub(super) fn refresh_directory(&mut self) -> Task<Message> {
        self.request_path(self.directory_path.clone(), HistoryRequest::Refresh)
    }

    pub(super) fn run_search(&mut self) -> Task<Message> {
        self.cancel_search();
        let Some(search) = &self.search else {
            return Task::none();
        };
        self.status = format!("Searching {}", search.root.display());
        let starting_depth = if search.depth == SearchDepth::Progressive {
            0
        } else {
            search.depth.max_depth()
        };
        self.dispatch_search(starting_depth, None)
    }

    /// Issues one search request at `requested_depth`. For a progressive
    /// search this is one step of many; `previous_count` is the number of
    /// results the prior step found, used to detect when going deeper stops
    /// turning up anything new.
    pub(super) fn dispatch_search(
        &mut self,
        requested_depth: u32,
        previous_count: Option<usize>,
    ) -> Task<Message> {
        let Some(search) = &mut self.search else {
            return Task::none();
        };
        search.in_progress = true;
        search.pending_request = Some((requested_depth, previous_count));
        let root = search.root.clone();
        let query = search.query.clone();
        let depth = search.depth;
        let (task, handle) = Task::perform(
            search_directory(root.clone(), query.clone(), requested_depth),
            move |result| Message::SearchFinished {
                root: root.clone(),
                query: query.clone(),
                depth,
                requested_depth,
                result,
            },
        )
        .abortable();
        search.cancel_handle = Some(handle);
        task
    }

    pub(super) fn cancel_search(&mut self) {
        if let Some(search) = &mut self.search
            && let Some(handle) = search.cancel_handle.take()
        {
            handle.abort();
        }
        if let Some(search) = &mut self.search {
            search.in_progress = false;
            search.pending_request = None;
        }
    }

    pub(super) fn apply_response(
        &mut self,
        result: Result<BrowseResponse, String>,
        history: HistoryRequest,
    ) -> Task<Message> {
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                self.status = error;
                return Task::none();
            }
        };

        match response.payload {
            Some(Payload::Directory(directory)) => {
                self.last_folder_load_duration = self
                    .folder_load_started
                    .take()
                    .map(|started| started.elapsed());
                self.folder_load_performance = Some(FolderLoadPerformance {
                    started_at: Instant::now(),
                    enumeration_ms: None,
                    expected_entries: 0,
                    first_item_at: None,
                    displayed_at: None,
                    thumbnails_total: 0,
                    thumbnails_settled: 0,
                    thumbnails_settled_at: None,
                    item_rendered_at: Vec::new(),
                    thumbnail_settled_at: Vec::new(),
                    entry_path_us: 0,
                    entry_symlink_us: 0,
                    entry_metadata_us: 0,
                    entry_timestamps_us: 0,
                });
                let preserve_thumbnails = matches!(history, HistoryRequest::Refresh)
                    && self.directory_path == PathBuf::from(&response.path);
                self.address = response.path.clone();
                self.directory_path = PathBuf::from(response.path);
                self.record_history(self.directory_path.clone(), history);
                let _ = directory;
                self.entries.clear();
                self.pending_thumbnail_paths.clear();
                self.selected_entries.clear();
                self.selection_anchor = None;
                if !preserve_thumbnails {
                    self.thumbnail_handles.clear();
                    self.refresh_entry_icons();
                }
                self.content.clear();
                self.status = "Loading folder contents".into();
                let directory = self.directory_path.clone();
                Task::run(stream_directory(directory.clone()), move |entry| {
                    Message::DirectoryEntriesLoaded {
                        directory: directory.clone(),
                        entries: entry,
                    }
                })
            }
            Some(Payload::File(file)) => {
                self.content = file.content;
                self.status = "File preview".into();
                Task::none()
            }
            Some(Payload::Error(error)) => {
                self.status = error.message;
                Task::none()
            }
            None => {
                self.status = "Backend returned an invalid response".into();
                Task::none()
            }
        }
    }

    pub(super) fn record_history(&mut self, path: PathBuf, request: HistoryRequest) {
        match request {
            HistoryRequest::Initial => {
                self.history = vec![path];
                self.history_index = Some(0);
            }
            HistoryRequest::New => {
                let Some(index) = self.history_index else {
                    self.history = vec![path];
                    self.history_index = Some(0);
                    return;
                };
                if self.history.get(index) == Some(&path) {
                    return;
                }
                self.history.truncate(index + 1);
                self.history.push(path);
                self.history_index = Some(self.history.len() - 1);
            }
            HistoryRequest::Existing(index) if self.history.get(index) == Some(&path) => {
                self.history_index = Some(index);
            }
            HistoryRequest::Existing(_) => self.record_history(path, HistoryRequest::New),
            HistoryRequest::Refresh => {}
        }
    }
}
