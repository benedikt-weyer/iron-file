use super::*;

impl Gui {
    pub(super) fn update_browser(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::AddressChanged(address) => {
                self.address = address;
                Task::none()
            }
            Message::StartAddressEdit => {
                self.editing_address = true;
                Task::none()
            }
            Message::CancelAddressEdit => {
                if !self.editing_address {
                    return Task::none();
                }
                self.address = self.directory_path.display().to_string();
                self.editing_address = false;
                Task::none()
            }
            Message::EscapePressed => {
                if self.context_entry.is_some() {
                    self.context_entry = None;
                } else if self.pending_info.is_some() {
                    self.pending_info = None;
                } else if self.pending_rename.is_some() {
                    self.pending_rename = None;
                    self.rename_entry_name.clear();
                } else if self.pending_create.is_some() {
                    self.pending_create = None;
                    self.create_entry_name.clear();
                } else if self.editing_address {
                    self.address = self.directory_path.display().to_string();
                    self.editing_address = false;
                }
                Task::none()
            }
            Message::OpenAddress => self.open_path(PathBuf::from(&self.address)),
            Message::OpenPath(path) => self.open_path(path),
            Message::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers;
                Task::none()
            }
            Message::StartRectangleSelection => {
                self.rectangle_selection = Some(RectangleSelection {
                    start: self.browser_pointer,
                    end: self.browser_pointer,
                    initial_selection: if self.modifiers.command() {
                        self.selected_entries.clone()
                    } else {
                        HashSet::new()
                    },
                });
                if !self.modifiers.command() {
                    self.selected_entries.clear();
                    self.selection_anchor = None;
                }
                Task::none()
            }
            Message::RectanglePointerMoved(position) => {
                self.browser_pointer = position;
                if let Some(selection) = &mut self.rectangle_selection {
                    selection.end = position;
                }
                self.update_rectangle_selection();
                Task::none()
            }
            Message::FinishRectangleSelection => {
                self.rectangle_selection = None;
                self.pending_drag = None;
                if let Some(target) = self.entry_drop_target.take() {
                    return self.drop_dragged_entries(target);
                }
                self.dragging_entries = None;
                Task::none()
            }
            Message::BrowserScrolled(viewport) => {
                self.browser_scroll_offset = viewport.absolute_offset();
                self.update_rectangle_selection();
                Task::none()
            }
            Message::NavigateBack => self.navigate_history(-1),
            Message::NavigateForward => self.navigate_history(1),
            Message::EntryClicked { path, is_directory } => {
                self.handle_entry_click(path, is_directory)
            }
            Message::EntryHovered { path, is_directory } => {
                self.hovered_entry = Some((path.clone(), is_directory));
                if is_directory && self.dragging_entries.is_some() {
                    self.entry_drop_target = Some(path);
                }
                Task::none()
            }
            Message::EntryUnhovered(path) => {
                if self.hovered_entry.as_ref().is_some_and(|(p, _)| p == &path) {
                    self.hovered_entry = None;
                }
                if self.entry_drop_target.as_ref() == Some(&path) {
                    self.entry_drop_target = None;
                }
                Task::none()
            }
            Message::LeftMouseButtonPressed => {
                if self.view == View::Browser && !self.editing_address {
                    if let Some((path, _)) = self.hovered_entry.clone() {
                        let entries = if self.selected_entries.contains(&path) {
                            self.selected_entries.iter().cloned().collect()
                        } else {
                            vec![path]
                        };
                        self.pending_drag = Some((self.window_cursor_position, entries));
                    }
                }
                Task::none()
            }
            Message::LeftMouseButtonReleased => {
                self.pending_drag = None;
                if let Some(target) = self.entry_drop_target.take() {
                    return self.drop_dragged_entries(target);
                }
                self.dragging_entries = None;
                Task::none()
            }
            Message::WindowCursorMoved(position) => {
                self.window_cursor_position = position;
                if let Some((start, entries)) = &self.pending_drag {
                    let delta = *start - position;
                    if delta.x.hypot(delta.y) > DRAG_START_THRESHOLD {
                        self.dragging_entries = Some(entries.clone());
                        self.pending_drag = None;
                    }
                }
                Task::none()
            }
            Message::ConfirmPicker => self.confirm_picker(),
            Message::CancelPicker => self.close_window(),
            Message::SaveFileNameChanged(name) => {
                self.save_file_name = Some(name);
                Task::none()
            }
            Message::ResetSaveFileName => {
                self.save_file_name = self.original_save_file_name.clone();
                Task::none()
            }
            Message::RefreshDirectory => self.refresh_directory(),
            Message::CloneWindow => {
                self.status = match clone_window(&self.directory_path) {
                    Ok(()) => "Opened new window".into(),
                    Err(error) => error,
                };
                Task::none()
            }
            Message::DuplicateContextEntry(path) => {
                let Some(parent) = path.parent().map(Path::to_path_buf) else {
                    self.status = format!("Could not duplicate {}", path.display());
                    return Task::none();
                };
                Task::perform(copy_entries(vec![path], parent), Message::FileCopyFinished)
            }
            Message::ExecuteBrowserCommand(command) => self.execute_browser_command(command),
            Message::FileCopyFinished(result) => match result {
                Ok(paths) => {
                    self.status = format!("Copied {} item(s)", paths.len());
                    self.open_path(self.directory_path.clone())
                }
                Err(error) => {
                    self.status = format!("Copy failed: {error}");
                    Task::none()
                }
            },
            Message::CutPasteFinished { sources, result } => match result {
                Ok(paths) => {
                    self.paste_buffer = None;
                    self.status = format!("Moved {} item(s)", paths.len());
                    Task::perform(delete_entries(sources), Message::FileDeleteFinished)
                }
                Err(error) => {
                    self.status = format!("Move failed: {error}");
                    Task::none()
                }
            },
            Message::ArchiveFinished { action, result } => match result {
                Ok(paths) => {
                    self.status = format!("{action} {} item(s)", paths.len());
                    self.open_path(self.directory_path.clone())
                }
                Err(error) => {
                    self.status = format!("{action} failed: {error}");
                    Task::none()
                }
            },
            Message::CompressionLevelChanged(level) => {
                self.compression_level = level;
                Task::none()
            }
            Message::CompressionTypeSelected(compression_type) => {
                self.compression_type = compression_type;
                Task::none()
            }
            Message::ConfirmCompression => {
                self.pending_compression = false;
                self.execute_compression()
            }
            Message::CancelCompression => {
                self.pending_compression = false;
                Task::none()
            }
            Message::ConfirmDelete => {
                let Some(paths) = self.pending_delete.take() else {
                    return Task::none();
                };
                self.delete_confirm_selected = false;
                self.status = format!("Deleting {} item(s)...", paths.len());
                Task::perform(delete_entries(paths), Message::FileDeleteFinished)
            }
            Message::CancelDelete => {
                self.pending_delete = None;
                self.delete_confirm_selected = false;
                Task::none()
            }
            Message::SelectDeleteDialogAction(delete) => {
                if self.pending_delete.is_some() {
                    self.delete_confirm_selected = delete;
                }
                Task::none()
            }
            Message::ActivateDeleteDialogAction => {
                if self.pending_delete.is_some() {
                    if self.delete_confirm_selected {
                        self.update(Message::ConfirmDelete)
                    } else {
                        self.update(Message::CancelDelete)
                    }
                } else {
                    Task::none()
                }
            }
            Message::ArrowKeyPressed(direction) => {
                if self.pending_delete.is_some() {
                    match direction {
                        SelectionDirection::Left => {
                            self.update(Message::SelectDeleteDialogAction(false))
                        }
                        SelectionDirection::Right => {
                            self.update(Message::SelectDeleteDialogAction(true))
                        }
                        SelectionDirection::Up | SelectionDirection::Down => Task::none(),
                    }
                } else {
                    self.move_selected_entry(direction);
                    Task::none()
                }
            }
            Message::FileDeleteFinished(result) => match result {
                Ok(paths) => {
                    self.status = format!("Deleted {} item(s)", paths.len());
                    self.selected_entries.clear();
                    self.selection_anchor = None;
                    self.open_path(self.directory_path.clone())
                }
                Err(error) => {
                    self.status = format!("Delete failed: {error}");
                    Task::none()
                }
            },
            Message::RequestCreateEntry {
                parent,
                is_directory,
            } => {
                self.context_entry = None;
                self.pending_create = Some((parent, is_directory));
                self.create_entry_name.clear();
                let id = create_name_input_id();
                operation::focus(id)
            }
            Message::CreateEntryNameChanged(name) => {
                self.create_entry_name = name;
                Task::none()
            }
            Message::ConfirmCreateEntry => {
                let Some((parent, is_directory)) = self.pending_create.take() else {
                    return Task::none();
                };
                let name = self.create_entry_name.trim().to_owned();
                if name.is_empty() {
                    self.pending_create = Some((parent, is_directory));
                    self.status = "Enter a name".into();
                    return Task::none();
                }
                self.status = format!("Creating {name}...");
                Task::perform(
                    create_entry(parent, name, is_directory),
                    Message::EntryCreated,
                )
            }
            Message::CancelCreateEntry => {
                self.pending_create = None;
                self.create_entry_name.clear();
                Task::none()
            }
            Message::EntryCreated(result) => match result {
                Ok(path) => {
                    self.create_entry_name.clear();
                    self.status = format!("Created {}", path.display());
                    self.open_path(self.directory_path.clone())
                }
                Err(error) => {
                    self.status = format!("Create failed: {error}");
                    Task::none()
                }
            },
            Message::RequestRenameEntry(path) => {
                self.context_entry = None;
                self.rename_entry_name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.pending_rename = Some(path);
                let id = rename_name_input_id();
                Task::batch([operation::focus(id.clone()), operation::select_all(id)])
            }
            Message::RenameEntryNameChanged(name) => {
                self.rename_entry_name = name;
                Task::none()
            }
            Message::ConfirmRenameEntry => {
                let Some(path) = self.pending_rename.take() else {
                    return Task::none();
                };
                let name = self.rename_entry_name.trim().to_owned();
                if name.is_empty() {
                    self.pending_rename = Some(path);
                    self.status = "Enter a name".into();
                    return Task::none();
                }
                self.status = format!("Renaming {}...", path.display());
                Task::perform(rename_entry(path, name), Message::EntryRenamed)
            }
            Message::CancelRenameEntry => {
                self.pending_rename = None;
                self.rename_entry_name.clear();
                Task::none()
            }
            Message::EntryRenamed(result) => match result {
                Ok(path) => {
                    self.rename_entry_name.clear();
                    self.status = format!("Renamed to {}", path.display());
                    self.open_path(self.directory_path.clone())
                }
                Err(error) => {
                    self.status = format!("Rename failed: {error}");
                    Task::none()
                }
            },
            Message::OpenParent => {
                let parent = self.directory_path.parent().map(|path| path.to_path_buf());
                parent
                    .map(|path| self.open_path(path))
                    .unwrap_or_else(Task::none)
            }
            Message::ShowSearch => {
                self.search = Some(SearchState {
                    root: self.directory_path.clone(),
                    query: String::new(),
                    depth: SearchDepth::CurrentFolder,
                    in_progress: false,
                    pending_request: None,
                    cancel_handle: None,
                });
                self.status = format!("Search in {}", self.directory_path.display());
                Task::none()
            }
            Message::CloseSearch => {
                self.cancel_search();
                self.search = None;
                self.refresh_directory()
            }
            Message::SearchQueryChanged(query) => {
                if let Some(search) = &mut self.search {
                    search.query = query;
                }
                self.run_search()
            }
            Message::SearchDepthSelected(depth) => {
                if let Some(search) = &mut self.search {
                    search.depth = depth;
                }
                self.run_search()
            }
            Message::RunSearch => self.run_search(),
            Message::CancelSearch => {
                self.cancel_search();
                self.status = "Search cancelled".into();
                Task::none()
            }
            Message::SearchFinished {
                root,
                query,
                depth,
                requested_depth,
                result,
            } => {
                let Some(search) = self.search.as_mut() else {
                    return Task::none();
                };
                let is_current = search.root == root
                    && search.query == query
                    && search.depth == depth
                    && search
                        .pending_request
                        .is_some_and(|(pending_depth, _)| pending_depth == requested_depth);
                if !is_current {
                    return Task::none();
                }
                match result {
                    Ok(mut entries) => {
                        let sort_order = self
                            .folder_sort_override(&self.directory_path)
                            .unwrap_or(self.active_browser_settings().sort_order);
                        sort_entries(&mut entries, sort_order);
                        let found = entries.len();
                        self.entries = entries;
                        self.selected_entries.clear();
                        self.selection_anchor = None;

                        let previous_count = self
                            .search
                            .as_ref()
                            .and_then(|s| s.pending_request)
                            .and_then(|(_, previous_count)| previous_count);
                        let keep_going = depth == SearchDepth::Progressive
                            && requested_depth < PROGRESSIVE_SEARCH_MAX_DEPTH
                            && previous_count != Some(found);
                        if keep_going {
                            self.status =
                                format!("{found} result(s) so far (depth {requested_depth})...");
                            return self.dispatch_search(requested_depth + 1, Some(found));
                        }

                        if let Some(search) = self.search.as_mut() {
                            search.in_progress = false;
                            search.pending_request = None;
                            search.cancel_handle = None;
                        }
                        self.status = format!("{found} search result(s)");
                        Task::none()
                    }
                    Err(error) => {
                        if let Some(search) = self.search.as_mut() {
                            search.in_progress = false;
                            search.pending_request = None;
                            search.cancel_handle = None;
                        }
                        self.status = format!("Search failed: {error}");
                        Task::none()
                    }
                }
            }
            other => self.update_preferences(other),
        }
    }
}
