use super::*;

impl Gui {
    pub(super) fn update_loading(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::BackendLogPipeEnded(Err(error)) => {
                self.status = format!("Backend log stream stopped: {error}");
                Task::none()
            }
            Message::BackendLogPipeEnded(Ok(())) => Task::none(),
            Message::RestartBackend => {
                self.status = "Restarting backend".into();
                Task::perform(restart_backend(), Message::BackendRestarted)
            }
            Message::BackendRestarted(result) => {
                self.status = match result {
                    Ok(()) => "Backend restarted".into(),
                    Err(error) => format!("Could not restart backend: {error}"),
                };
                Task::none()
            }
            Message::ThumbnailGenerated {
                path,
                thumbnail_path: Ok(thumbnail_path),
            } => {
                if let Some(performance) = &mut self.folder_load_performance {
                    performance.thumbnails_settled += 1;
                    performance.thumbnail_settled_at.push(Instant::now());
                    if performance.displayed_at.is_some()
                        && performance.thumbnails_settled == performance.thumbnails_total
                    {
                        performance.thumbnails_settled_at = Some(Instant::now());
                    }
                }
                if !thumbnail_path.is_empty()
                    && let Some(entry) = self
                        .entries
                        .iter_mut()
                        .find(|entry| PathBuf::from(&entry.path) == path)
                {
                    entry.thumbnail_path = thumbnail_path;
                }
                if let Some(entry) = self
                    .entries
                    .iter()
                    .find(|entry| PathBuf::from(&entry.path) == path)
                    && let Ok(bytes) = fs::read(&entry.thumbnail_path)
                {
                    self.thumbnail_handles
                        .insert(path, image::Handle::from_bytes(bytes));
                }
                Task::none()
            }
            Message::ThumbnailGenerated {
                thumbnail_path: Err(error),
                ..
            } => {
                if let Some(performance) = &mut self.folder_load_performance {
                    performance.thumbnails_settled += 1;
                    performance.thumbnail_settled_at.push(Instant::now());
                }
                eprintln!("[iron-file thumbnails] {error}");
                Task::none()
            }
            Message::DirectoryEntriesLoaded { directory, entries } => match entries {
                Ok(entries) if self.directory_path == directory => {
                    let icon_theme = self.active_browser_settings().icon_theme;
                    let icon_names = gio_icon_names_batch(
                        &entries
                            .iter()
                            .filter(|entry| !entry.directory_complete)
                            .map(|entry| PathBuf::from(&entry.path))
                            .collect::<Vec<_>>(),
                    );
                    let mut completed = None;
                    for entry in entries {
                        if entry.directory_complete {
                            completed = Some(entry);
                            continue;
                        }
                        let path = PathBuf::from(&entry.path);
                        let now = Instant::now();
                        if let Some(performance) = &mut self.folder_load_performance {
                            performance.first_item_at.get_or_insert(now);
                            performance.item_rendered_at.push(now);
                            performance.entry_path_us += entry.entry_path_ms;
                            performance.entry_symlink_us += entry.entry_symlink_ms;
                            performance.entry_metadata_us += entry.entry_metadata_ms;
                            performance.entry_timestamps_us += entry.entry_timestamps_ms;
                        }
                        let icon = self.cached_entry_icon_path_with_names(
                            &icon_theme,
                            &entry,
                            icon_names.get(&path),
                        );
                        self.entry_icons.insert(path.clone(), icon);
                        if !entry.is_directory && !self.thumbnail_handles.contains_key(&path) {
                            if let Some(performance) = &mut self.folder_load_performance {
                                performance.thumbnails_total += 1;
                            }
                            self.pending_thumbnail_paths.push(path);
                        }
                        self.entries.push(entry);
                    }
                    let sort_order = self.current_sort_order();
                    sort_entries(&mut self.entries, sort_order);
                    self.status = format!("{} entries", self.entries.len());
                    if let Some(entry) = completed {
                        if let Some(performance) = &mut self.folder_load_performance {
                            performance.enumeration_ms = Some(entry.directory_enumeration_ms);
                            performance.expected_entries = entry.directory_entry_count as usize;
                            performance.displayed_at = Some(Instant::now());
                        }
                        let thumbnail_directory = self.active_browser_settings().thumbnail_location;
                        return Task::batch(self.pending_thumbnail_paths.drain(..).map(|path| {
                            Task::perform(
                                create_thumbnail(path.clone(), thumbnail_directory.clone()),
                                move |thumbnail_path| Message::ThumbnailGenerated {
                                    path: path.clone(),
                                    thumbnail_path,
                                },
                            )
                        }));
                    }
                    Task::none()
                }
                Ok(_) => Task::none(),
                Err(error) => self.update(Message::DirectoryEntryLoaded {
                    directory,
                    entry: Err(error),
                }),
            },
            Message::DirectoryEntryLoaded {
                directory,
                entry: Ok(entry),
            } => {
                if self.directory_path != directory {
                    return Task::none();
                }
                if entry.directory_complete {
                    if let Some(performance) = &mut self.folder_load_performance {
                        performance.enumeration_ms = Some(entry.directory_enumeration_ms);
                        performance.expected_entries = entry.directory_entry_count as usize;
                        performance.displayed_at = Some(Instant::now());
                        if performance.thumbnails_total == performance.thumbnails_settled {
                            performance.thumbnails_settled_at = Some(Instant::now());
                        }
                    }
                    let thumbnail_directory = self.active_browser_settings().thumbnail_location;
                    return Task::batch(self.pending_thumbnail_paths.drain(..).map(|path| {
                        Task::perform(
                            create_thumbnail(path.clone(), thumbnail_directory.clone()),
                            move |thumbnail_path| Message::ThumbnailGenerated {
                                path: path.clone(),
                                thumbnail_path,
                            },
                        )
                    }));
                }
                let path = PathBuf::from(&entry.path);
                if let Some(performance) = &mut self.folder_load_performance
                    && performance.first_item_at.is_none()
                {
                    performance.first_item_at = Some(Instant::now());
                }
                if let Some(performance) = &mut self.folder_load_performance {
                    performance.item_rendered_at.push(Instant::now());
                    performance.entry_path_us += entry.entry_path_ms;
                    performance.entry_symlink_us += entry.entry_symlink_ms;
                    performance.entry_metadata_us += entry.entry_metadata_ms;
                    performance.entry_timestamps_us += entry.entry_timestamps_ms;
                }
                let icon_theme = self.active_browser_settings().icon_theme;
                let icon = self.cached_entry_icon_path(&icon_theme, &entry);
                self.entry_icons.insert(path.clone(), icon);
                let is_directory = entry.is_directory;
                let thumbnail_is_loaded = self.thumbnail_handles.contains_key(&path);
                let sort_order = self.current_sort_order();
                self.entries.push(entry);
                sort_entries(&mut self.entries, sort_order);
                self.status = format!("{} entries", self.entries.len());
                if is_directory || thumbnail_is_loaded {
                    Task::none()
                } else {
                    if let Some(performance) = &mut self.folder_load_performance {
                        performance.thumbnails_total += 1;
                    }
                    self.pending_thumbnail_paths.push(path);
                    Task::none()
                }
            }
            Message::DirectoryEntryLoaded {
                directory,
                entry: Err(error),
            } => {
                if self.directory_path == directory {
                    self.status = format!("Could not load folder contents: {error}");
                }
                Task::none()
            }
            Message::BrowseFinished { result, history } => self.apply_response(result, history),
            Message::IconFontLoaded(_) => Task::none(),
            _ => unreachable!("message not routed by any update handler"),
        }
    }
}
