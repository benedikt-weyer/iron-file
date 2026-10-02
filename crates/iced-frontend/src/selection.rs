use super::*;

impl Gui {
    pub(super) fn handle_entry_click(
        &mut self,
        path: PathBuf,
        is_directory: bool,
    ) -> Task<Message> {
        if let Some(picker) = self.picker.as_ref() {
            let now = Instant::now();
            let is_double_click =
                self.last_entry_click
                    .as_ref()
                    .is_some_and(|(last_path, last_click)| {
                        last_path == &path
                            && now.duration_since(*last_click) <= Duration::from_millis(500)
                    });
            self.last_entry_click = Some((path.clone(), now));
            if is_directory && is_double_click {
                self.last_entry_click = None;
                return self.open_path(path);
            }
            if (picker.kind == PickerKind::Folder) == is_directory {
                self.select_entry(&path);
            }
            return Task::none();
        }
        self.select_entry(&path);
        let now = Instant::now();
        let is_double_click =
            self.last_entry_click
                .as_ref()
                .is_some_and(|(last_path, last_click)| {
                    last_path == &path
                        && now.duration_since(*last_click) <= Duration::from_millis(500)
                });
        self.last_entry_click = Some((path.clone(), now));

        if is_directory {
            if self.active_browser_settings().single_click_opens_folders || is_double_click {
                self.last_entry_click = None;
                self.open_path(path)
            } else {
                Task::none()
            }
        } else if is_double_click {
            self.last_entry_click = None;
            Task::perform(open_file(path), Message::FileOpened)
        } else {
            self.open_path(path)
        }
    }

    pub(super) fn select_entry(&mut self, path: &Path) {
        if self.picker.as_ref().is_some_and(|picker| !picker.multiple) {
            self.selected_entries.clear();
            self.selected_entries.insert(path.to_path_buf());
            self.selection_anchor = Some(path.to_path_buf());
            return;
        }
        let add_to_selection = self.modifiers.command();
        let range_selection = self.modifiers.shift();
        if range_selection {
            let anchor = self.selection_anchor.as_deref().unwrap_or(path);
            let anchor_index = self
                .entries
                .iter()
                .position(|entry| Path::new(&entry.path) == anchor);
            let target_index = self
                .entries
                .iter()
                .position(|entry| Path::new(&entry.path) == path);
            if let (Some(anchor_index), Some(target_index)) = (anchor_index, target_index) {
                if !add_to_selection {
                    self.selected_entries.clear();
                }
                let (start, end) = if anchor_index <= target_index {
                    (anchor_index, target_index)
                } else {
                    (target_index, anchor_index)
                };
                self.selected_entries.extend(
                    self.entries[start..=end]
                        .iter()
                        .map(|entry| PathBuf::from(&entry.path)),
                );
            }
        } else if add_to_selection {
            if !self.selected_entries.insert(path.to_path_buf()) {
                self.selected_entries.remove(path);
                if self.selection_anchor.as_deref() == Some(path) {
                    self.selection_anchor = None;
                }
            } else {
                self.selection_anchor = Some(path.to_path_buf());
            }
        } else {
            self.selected_entries.clear();
            self.selected_entries.insert(path.to_path_buf());
            self.selection_anchor = Some(path.to_path_buf());
        }
        if range_selection {
            self.selection_anchor = Some(path.to_path_buf());
        }
    }

    pub(super) fn move_selected_entry(&mut self, direction: SelectionDirection) {
        if self.view != View::Browser
            || self.editing_address
            || self.pending_create.is_some()
            || self.pending_rename.is_some()
            || self.pending_compression
            || self.selected_entries.len() != 1
        {
            return;
        }
        let browser = self.active_browser_settings();
        let entries = self
            .entries
            .iter()
            .filter(|entry| browser.show_hidden_files || !entry.name.starts_with('.'))
            .collect::<Vec<_>>();
        let Some(index) = entries
            .iter()
            .position(|entry| self.selected_entries.contains(Path::new(&entry.path)))
        else {
            return;
        };
        let target = match (browser.layout, direction) {
            (_, SelectionDirection::Left | SelectionDirection::Up)
                if browser.layout == BrowserLayout::List =>
            {
                index.checked_sub(1)
            }
            (_, SelectionDirection::Right | SelectionDirection::Down)
                if browser.layout == BrowserLayout::List =>
            {
                (index + 1 < entries.len()).then_some(index + 1)
            }
            (_, SelectionDirection::Left) => index.checked_sub(1),
            (_, SelectionDirection::Right) => (index + 1 < entries.len()).then_some(index + 1),
            (_, SelectionDirection::Up) => index.checked_sub(self.tile_columns.get().max(1)),
            (_, SelectionDirection::Down) => (index + self.tile_columns.get().max(1)
                < entries.len())
            .then_some(index + self.tile_columns.get().max(1)),
        };
        let Some(target) = target else {
            return;
        };
        let path = PathBuf::from(&entries[target].path);
        self.selected_entries.clear();
        self.selected_entries.insert(path.clone());
        self.selection_anchor = Some(path);
        self.last_entry_click = None;
    }

    pub(super) fn update_rectangle_selection(&mut self) {
        let Some(selection) = self.rectangle_selection.clone() else {
            return;
        };

        let offset = self.browser_scroll_offset;
        let left = selection.start.x.min(selection.end.x) + offset.x;
        let right = selection.start.x.max(selection.end.x) + offset.x;
        let top = selection.start.y.min(selection.end.y) + offset.y;
        let bottom = selection.start.y.max(selection.end.y) + offset.y;
        let browser = self.active_browser_settings();
        let tile_width = f32::from(browser.item_size) * 3.5;
        let tile_height = tile_width * 1.2;
        let row_height = f32::from(browser.item_size).max(24.0) + 12.0;
        let columns = self.tile_columns.get().max(1);

        self.selected_entries = selection.initial_selection;
        for (index, entry) in self
            .entries
            .iter()
            .filter(|entry| browser.show_hidden_files || !entry.name.starts_with('.'))
            .enumerate()
        {
            let (x, y, width, height) = if browser.layout == BrowserLayout::Tiles {
                let column = index % columns;
                let row = index / columns;
                (
                    column as f32 * (tile_width + 8.0),
                    row as f32 * (tile_height + 8.0),
                    tile_width,
                    tile_height,
                )
            } else {
                (0.0, index as f32 * row_height, f32::INFINITY, row_height)
            };
            if x < right && x + width > left && y < bottom && y + height > top {
                self.selected_entries.insert(PathBuf::from(&entry.path));
            }
        }
    }
}
