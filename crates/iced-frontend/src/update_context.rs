use super::*;

impl Gui {
    pub(super) fn update_context(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::StartSidebarResize => {
                self.sidebar_resize = Some((
                    self.pointer_position.x,
                    self.sidebar_width(),
                    self.sidebar_width(),
                ));
                Task::none()
            }
            Message::FinishSidebarResize => {
                let Some((_, _, sidebar_width)) = self.sidebar_resize.take() else {
                    return Task::none();
                };
                let mut browser = self.active_browser_settings();
                browser.sidebar_width = sidebar_width;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::ShowEntryContext { path, is_directory } => {
                self.selected_entries.clear();
                self.selected_entries.insert(path.clone());
                self.selection_anchor = Some(path.clone());
                self.context_entry = Some(ContextEntry {
                    path: path.clone(),
                    is_directory,
                    is_sidebar_location: false,
                    opener: None,
                    open_with_expanded: false,
                    open_with_apps: None,
                });
                self.context_position = self.pointer_position;
                if is_directory {
                    Task::none()
                } else {
                    Task::perform(default_file_opener(path.clone()), move |opener| {
                        Message::FileOpenerResolved {
                            path: path.clone(),
                            opener,
                        }
                    })
                }
            }
            Message::ShowSidebarLocationContext(path) => {
                self.context_entry = Some(ContextEntry {
                    path,
                    is_directory: true,
                    is_sidebar_location: true,
                    opener: None,
                    open_with_expanded: false,
                    open_with_apps: None,
                });
                self.context_position = self.pointer_position;
                Task::none()
            }
            Message::SetSidebarLocationIcon { path, icon } => {
                self.set_sidebar_location_icon(path, icon);
                Task::none()
            }
            Message::FileOpenerResolved { path, opener } => {
                if let Some(context_entry) = &mut self.context_entry
                    && !context_entry.is_directory
                    && context_entry.path == path
                {
                    context_entry.opener = Some(opener);
                }
                Task::none()
            }
            Message::ContextPointerMoved(position) => {
                self.pointer_position = position;
                if let Some((start_x, initial_width, _)) = self.sidebar_resize {
                    let sidebar_width = (f32::from(initial_width) + position.x - start_x)
                        .round()
                        .clamp(140.0, 600.0) as u16;
                    self.sidebar_resize = Some((start_x, initial_width, sidebar_width));
                }
                Task::none()
            }
            Message::CloseFolderContext => {
                self.context_entry = None;
                Task::none()
            }
            Message::TogglePerformanceDebugger => {
                self.show_performance_debugger = !self.show_performance_debugger;
                Task::none()
            }
            Message::CopyPerformanceReport => {
                let Some(load) = &self.folder_load_performance else {
                    self.status = "No folder-load performance data to copy".into();
                    return Task::none();
                };
                let displayed_ms = load
                    .displayed_at
                    .map(|at| at.duration_since(load.started_at).as_millis())
                    .unwrap_or_default();
                let thumbnails_ms = load
                    .thumbnails_settled_at
                    .map(|at| at.duration_since(load.started_at).as_millis())
                    .unwrap_or_default();
                let item_milestones = [10, 25, 50, 75, 90]
                    .into_iter()
                    .filter_map(|percent| {
                        let index = load
                            .expected_entries
                            .saturating_mul(percent)
                            .div_ceil(100)
                            .saturating_sub(1);
                        load.item_rendered_at.get(index).map(|at| {
                            format!(
                                "  {percent}%: {} ms",
                                at.duration_since(load.started_at).as_millis()
                            )
                        })
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut item_intervals = load
                    .item_rendered_at
                    .windows(2)
                    .map(|timestamps| {
                        timestamps[1].duration_since(timestamps[0]).as_secs_f64() * 1_000.0
                    })
                    .collect::<Vec<_>>();
                item_intervals.sort_by(f64::total_cmp);
                let item_render_stats = if item_intervals.is_empty() {
                    "Item render intervals: unavailable".to_owned()
                } else {
                    let average = item_intervals.iter().sum::<f64>() / item_intervals.len() as f64;
                    let percentile =
                        |percent: usize| item_intervals[(item_intervals.len() - 1) * percent / 100];
                    format!(
                        "Item render interval: avg {average:.2} ms | min {:.2} ms | p5 {:.2} ms | p95 {:.2} ms | max {:.2} ms",
                        item_intervals[0],
                        percentile(5),
                        percentile(95),
                        item_intervals[item_intervals.len() - 1]
                    )
                };
                let entry_count = load.item_rendered_at.len().max(1) as f64;
                let backend_item_averages = format!(
                    "Average backend item steps: path {:.3} ms | symlink {:.3} ms | metadata {:.3} ms | timestamps {:.3} ms",
                    load.entry_path_us as f64 / entry_count / 1_000.0,
                    load.entry_symlink_us as f64 / entry_count / 1_000.0,
                    load.entry_metadata_us as f64 / entry_count / 1_000.0,
                    load.entry_timestamps_us as f64 / entry_count / 1_000.0,
                );
                self.status = "Copied performance report to the clipboard".into();
                iced::clipboard::write(format!(
                    "Folder load: {}\nBrowse response: {} ms\nBackend enumeration: {} ms\nFirst item rendered: {} ms\nAll items displayed: {} ms\nAll thumbnails loaded: {} ms ({}/{})\nEntries: {}\n\nItem render milestones:\n{}\n\n{}\n{}",
                    self.directory_path.display(),
                    self.last_folder_load_duration
                        .map(|duration| duration.as_millis())
                        .unwrap_or_default(),
                    load.enumeration_ms.unwrap_or_default(),
                    load.first_item_at
                        .map(|at| at.duration_since(load.started_at).as_millis())
                        .unwrap_or_default(),
                    displayed_ms,
                    thumbnails_ms,
                    load.thumbnails_settled,
                    load.thumbnails_total,
                    load.expected_entries,
                    item_milestones,
                    item_render_stats,
                    backend_item_averages,
                ))
            }
            Message::RequestEntryInfo(path) => {
                self.context_entry = None;
                self.pending_info = Some(InfoDialog::Loading(path.clone()));
                Task::perform(inspect_entry(path.clone()), move |result| {
                    Message::EntryInfoLoaded {
                        path: path.clone(),
                        result,
                    }
                })
            }
            Message::EntryInfoLoaded { path, result } => {
                if !matches!(self.pending_info, Some(InfoDialog::Loading(ref current)) if current == &path)
                {
                    return Task::none();
                }
                self.pending_info = Some(match result {
                    Ok(info) => InfoDialog::Loaded(EntryInfo {
                        path: path.clone(),
                        name: info.name,
                        rows: info
                            .fields
                            .into_iter()
                            .map(|field| (field.label, field.value))
                            .collect(),
                    }),
                    Err(error) => InfoDialog::Error { path, error },
                });
                Task::none()
            }
            Message::CloseEntryInfo => {
                self.pending_info = None;
                Task::none()
            }
            Message::OpenContextFile => {
                let Some(context_entry) = self.context_entry.take() else {
                    return Task::none();
                };
                Task::perform(open_file(context_entry.path), Message::FileOpened)
            }
            Message::ToggleOpenWith => {
                let Some(context_entry) = &mut self.context_entry else {
                    return Task::none();
                };
                context_entry.open_with_expanded = !context_entry.open_with_expanded;
                if context_entry.open_with_expanded && context_entry.open_with_apps.is_none() {
                    let path = context_entry.path.clone();
                    Task::perform(list_open_with_apps(path.clone()), move |apps| {
                        Message::OpenWithAppsLoaded {
                            path: path.clone(),
                            apps,
                        }
                    })
                } else {
                    Task::none()
                }
            }
            Message::OpenWithAppsLoaded { path, apps } => {
                if let Some(context_entry) = &mut self.context_entry
                    && context_entry.path == path
                {
                    context_entry.open_with_apps = Some(apps);
                }
                Task::none()
            }
            Message::OpenWithApp(app_id) => {
                let Some(context_entry) = self.context_entry.take() else {
                    return Task::none();
                };
                Task::perform(
                    open_with_app(context_entry.path, app_id),
                    Message::FileOpened,
                )
            }
            Message::SetDefaultApp(app_id) => {
                let Some(context_entry) = &self.context_entry else {
                    return Task::none();
                };
                let path = context_entry.path.clone();
                Task::perform(
                    set_default_app_for_path(path.clone(), app_id),
                    move |result| Message::DefaultAppSet {
                        path: path.clone(),
                        result,
                    },
                )
            }
            Message::DefaultAppSet { path, result } => {
                self.status = match &result {
                    Ok(()) => "Default application updated".into(),
                    Err(error) => error.clone(),
                };
                let Some(context_entry) = &mut self.context_entry else {
                    return Task::none();
                };
                if context_entry.path != path {
                    return Task::none();
                }
                context_entry.open_with_apps = None;
                context_entry.opener = None;
                Task::perform(default_file_opener(path.clone()), move |opener| {
                    Message::FileOpenerResolved {
                        path: path.clone(),
                        opener,
                    }
                })
            }
            Message::OpenTerminalHere => {
                let Some(ContextEntry {
                    path,
                    is_directory: true,
                    ..
                }) = self.context_entry.take()
                else {
                    return Task::none();
                };
                let command = self.active_browser_settings().terminal_command;
                Task::perform(open_terminal(path, command), Message::TerminalOpened)
            }
            Message::AddContextFolderToSidebar => {
                self.add_context_folder_to_sidebar();
                Task::none()
            }
            Message::RemoveContextFolderFromSidebar => {
                self.remove_context_folder_from_sidebar();
                Task::none()
            }
            Message::SidebarPressed(path) => {
                self.sidebar_resize = None;
                self.dragging_sidebar_location = Some(path);
                self.sidebar_drop_target = None;
                self.sidebar_drop_at_end = false;
                Task::none()
            }
            Message::SidebarReleased(path) => {
                if self.dragging_entries.is_some() {
                    self.sidebar_drop_target = None;
                    self.drop_dragged_entries(path)
                } else {
                    self.release_sidebar_location(path)
                }
            }
            Message::SidebarDragTarget(path) => {
                if self.dragging_sidebar_location.is_some() || self.dragging_entries.is_some() {
                    self.sidebar_drop_target = Some(path);
                }
                Task::none()
            }
            Message::SidebarDragTargetCleared(path) => {
                if self.sidebar_drop_target.as_ref() == Some(&path) {
                    self.sidebar_drop_target = None;
                }
                Task::none()
            }
            Message::SidebarDragTargetEnd => {
                if self.dragging_sidebar_location.is_some() {
                    self.sidebar_drop_target = None;
                    self.sidebar_drop_at_end = true;
                }
                Task::none()
            }
            Message::SidebarDragTargetEndCleared => {
                self.sidebar_drop_at_end = false;
                Task::none()
            }
            Message::SidebarReleasedAtEnd => self.release_sidebar_location_at_end(),
            Message::MountsLoaded(result) => {
                match result {
                    Ok(state) => {
                        self.drives = state.drives;
                        self.mounts = state.mounts;
                    }
                    Err(error) => self.status = error,
                }
                Task::none()
            }
            Message::MountDrive(path) => {
                self.status = format!("Mounting {}", path.display());
                Task::perform(mount_drive(path), Message::MountsLoaded)
            }
            Message::FileOpened(result) => {
                self.status = match result {
                    Ok(()) => "Opened file".into(),
                    Err(error) => error,
                };
                Task::none()
            }
            Message::TerminalOpened(result) => {
                self.status = match result {
                    Ok(()) => "Opened terminal".into(),
                    Err(error) => error,
                };
                Task::none()
            }
            other => self.update_loading(other),
        }
    }
}
