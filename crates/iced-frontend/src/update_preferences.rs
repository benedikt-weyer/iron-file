use super::*;

impl Gui {
    pub(super) fn update_preferences(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ShowBrowser => {
                self.view = View::Browser;
                Task::none()
            }
            Message::ShowPreferences => {
                self.view = View::Preferences;
                Task::none()
            }
            Message::SelectProfile(path) => {
                self.select_profile(path);
                Task::none()
            }
            Message::NewProfileNameChanged(name) => {
                self.new_profile_name = name;
                Task::none()
            }
            Message::CreateProfile => {
                self.create_profile();
                Task::none()
            }
            Message::RequestProfileReset => {
                self.pending_profile_reset = true;
                Task::none()
            }
            Message::ConfirmProfileReset => {
                self.pending_profile_reset = false;
                self.reset_active_profile();
                Task::none()
            }
            Message::CancelProfileReset => {
                self.pending_profile_reset = false;
                Task::none()
            }
            Message::ResetPreference(option) => {
                self.reset_preference(option);
                Task::none()
            }
            Message::ColorModeSelected(color_mode) => {
                self.save_color_mode(color_mode);
                Task::none()
            }
            Message::BackgroundOpacityChanged(opacity) => {
                self.save_background_opacity(opacity);
                Task::none()
            }
            Message::ContextMenuBlurStrengthChanged(strength) => {
                self.save_context_menu_blur_strength(strength);
                Task::none()
            }
            Message::ContextMenuBlurKernelSizeChanged(kernel_size) => {
                self.save_context_menu_blur_kernel_size(kernel_size);
                Task::none()
            }
            Message::ContextMenuItemToggled {
                item,
                is_directory,
                enabled,
            } => {
                let browser = self.active_browser_settings();
                let mut items = if is_directory {
                    browser.folder_context_menu_items
                } else {
                    browser.file_context_menu_items
                };
                if enabled {
                    if !items.contains(&item) {
                        items.push(item);
                    }
                } else {
                    items.retain(|configured_item| *configured_item != item);
                }
                self.save_context_menu_items(is_directory, items);
                Task::none()
            }
            Message::MoveContextMenuItem {
                item,
                is_directory,
                move_up,
            } => {
                let browser = self.active_browser_settings();
                let mut items = if is_directory {
                    browser.folder_context_menu_items
                } else {
                    browser.file_context_menu_items
                };
                if let Some(index) = items
                    .iter()
                    .position(|configured_item| *configured_item == item)
                {
                    let target = if move_up {
                        index.checked_sub(1)
                    } else {
                        (index + 1 < items.len()).then_some(index + 1)
                    };
                    if let Some(target) = target {
                        items.swap(index, target);
                        self.save_context_menu_items(is_directory, items);
                    }
                }
                Task::none()
            }
            Message::QuickToolbarItemToggled(item, enabled) => {
                let mut items = self.active_browser_settings().quick_toolbar_items;
                if enabled {
                    if !items.contains(&item) {
                        items.push(item);
                    }
                } else {
                    items.retain(|configured_item| *configured_item != item);
                }
                self.save_quick_toolbar_items(items);
                Task::none()
            }
            Message::MoveQuickToolbarItem(item, move_up) => {
                let mut items = self.active_browser_settings().quick_toolbar_items;
                if let Some(index) = items
                    .iter()
                    .position(|configured_item| *configured_item == item)
                {
                    let target = if move_up {
                        index.checked_sub(1)
                    } else {
                        (index + 1 < items.len()).then_some(index + 1)
                    };
                    if let Some(target) = target {
                        items.swap(index, target);
                        self.save_quick_toolbar_items(items);
                    }
                }
                Task::none()
            }
            Message::SortOrderSelected(sort_order) => {
                let mut browser = self.active_browser_settings();
                browser.sort_order = sort_order;
                let effective_sort_order = self
                    .folder_sort_override(&self.directory_path)
                    .unwrap_or(sort_order);
                sort_entries(&mut self.entries, effective_sort_order);
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::FolderSortOverrideSelected(selection) => {
                self.save_folder_sort_override(selection.sort_order());
                Task::none()
            }
            Message::KeyboardShortcutChanged { action, key } => {
                self.save_keyboard_shortcut(action, key);
                Task::none()
            }
            Message::ShortcutPressed(key) => {
                let action = self
                    .active_browser_settings()
                    .keyboard_shortcuts
                    .into_iter()
                    .find(|shortcut| shortcut.key.eq_ignore_ascii_case(&key))
                    .map(|shortcut| shortcut.action);
                match action {
                    Some(KeyboardShortcutAction::RenameSelection) => {
                        self.execute_browser_command(BrowserCommand::RenameSelection)
                    }
                    Some(KeyboardShortcutAction::SearchCurrentFolder) => {
                        self.update(Message::ShowSearch)
                    }
                    None => Task::none(),
                }
            }
            Message::BorderRadiusChanged(radius) => {
                self.save_border_radius(radius);
                Task::none()
            }
            Message::OpenAccentPicker(dark) => {
                let color = parse_color(if dark {
                    &self.dark_accent_input
                } else {
                    &self.light_accent_input
                })
                .unwrap_or(Color::BLACK);
                let (hue, saturation, value) = rgb_to_hsv(color);
                self.accent_picker = Some(AccentPickerState {
                    dark,
                    hue,
                    saturation,
                    value,
                });
                Task::none()
            }
            Message::AccentHueChanged(hue) => {
                if let Some(picker) = &mut self.accent_picker {
                    picker.hue = hue;
                }
                Task::none()
            }
            Message::AccentSaturationChanged(saturation) => {
                if let Some(picker) = &mut self.accent_picker {
                    picker.saturation = saturation;
                }
                Task::none()
            }
            Message::AccentValueChanged(value) => {
                if let Some(picker) = &mut self.accent_picker {
                    picker.value = value;
                }
                Task::none()
            }
            Message::ConfirmAccentPicker => {
                let Some(picker) = self.accent_picker.take() else {
                    return Task::none();
                };
                let color = hsv_color(picker.hue, picker.saturation, picker.value);
                self.save_accent_color(
                    picker.dark,
                    format!(
                        "#{:02x}{:02x}{:02x}",
                        (color.r * 255.0).round() as u8,
                        (color.g * 255.0).round() as u8,
                        (color.b * 255.0).round() as u8
                    ),
                );
                Task::none()
            }
            Message::CancelAccentPicker => {
                self.accent_picker = None;
                Task::none()
            }
            Message::BrowserLayoutSelected(layout) => {
                let mut browser = self.active_browser_settings();
                browser.layout = layout;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::NameAlignmentSelected(name_alignment) => {
                let mut browser = self.active_browser_settings();
                browser.name_alignment = name_alignment;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::SmoothScrollingToggled(smooth_scrolling) => {
                let mut browser = self.active_browser_settings();
                browser.smooth_scrolling = smooth_scrolling;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::ScrollStepChanged(scroll_step) => {
                let mut browser = self.active_browser_settings();
                browser.scroll_step = scroll_step.clamp(10, 200);
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::BrowserItemSizeChanged(item_size) => {
                let mut browser = self.active_browser_settings();
                browser.item_size = item_size;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::MaxNameLinesChanged(max_name_lines) => {
                let mut browser = self.active_browser_settings();
                browser.max_name_lines = max_name_lines.clamp(1, 5);
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::PreviewToggled(preview_enabled) => {
                let mut browser = self.active_browser_settings();
                browser.preview_enabled = preview_enabled;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::ToggleHiddenFiles => {
                let mut browser = self.active_browser_settings();
                browser.show_hidden_files = !browser.show_hidden_files;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::SingleClickFoldersToggled(single_click_opens_folders) => {
                let mut browser = self.active_browser_settings();
                browser.single_click_opens_folders = single_click_opens_folders;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::TerminalChoiceSelected(choice) => {
                let mut browser = self.active_browser_settings();
                browser.terminal_command = if choice == DEFAULT_TERMINAL_CHOICE {
                    "default".into()
                } else if choice == CUSTOM_TERMINAL_CHOICE {
                    if browser.terminal_command == "default"
                        || self
                            .terminal_recommendations
                            .contains(&browser.terminal_command)
                    {
                        String::new()
                    } else {
                        browser.terminal_command
                    }
                } else {
                    choice
                };
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::TerminalCommandChanged(terminal_command) => {
                let mut browser = self.active_browser_settings();
                browser.terminal_command = terminal_command;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::IconThemeSelected(icon_theme) => {
                let mut browser = self.active_browser_settings();
                browser.icon_theme = icon_theme;
                self.save_browser_settings(browser);
                Task::none()
            }
            Message::ThumbnailLocationChanged(thumbnail_location) => {
                let mut browser = self.active_browser_settings();
                browser.thumbnail_location = PathBuf::from(thumbnail_location);
                self.save_browser_settings(browser);
                Task::none()
            }
            other => self.update_context(other),
        }
    }
}
