use super::*;
use iced::widget::text::Wrapping;
use iced::widget::{column, row, stack};

fn name_label_lines(name: &str, line_characters: usize, max_lines: usize) -> Vec<String> {
    let max_characters = line_characters.saturating_mul(max_lines);
    let label = truncate_label(name, max_characters);
    let characters = label.chars().collect::<Vec<_>>();
    characters
        .chunks(line_characters.max(1))
        .map(|line| line.iter().collect())
        .collect()
}

fn name_label(
    name: &str,
    line_characters: usize,
    max_lines: usize,
    _line_height: f32,
    alignment: iced::alignment::Horizontal,
) -> Element<'_, Message> {
    text(truncate_label(
        name,
        line_characters.saturating_mul(max_lines),
    ))
    .width(Length::Fill)
    .wrapping(Wrapping::Glyph)
    .align_x(alignment)
    .into()
}

impl Gui {
    pub(super) fn browser_view(&self) -> Element<'_, Message> {
        let browser_settings = self.active_browser_settings();
        let max_name_lines = usize::from(browser_settings.max_name_lines.clamp(1, 5));
        let name_alignment = match browser_settings.name_alignment {
            NameAlignment::Left => iced::alignment::Horizontal::Left,
            NameAlignment::Center => iced::alignment::Horizontal::Center,
            NameAlignment::Right => iced::alignment::Horizontal::Right,
        };
        let visible_entries = self
            .entries
            .iter()
            .filter(|entry| browser_settings.show_hidden_files || !entry.name.starts_with('.'))
            .collect::<Vec<_>>();
        let entries = visible_entries.iter().fold(column![], |column, entry| {
            let icon = self.entry_icon(entry, browser_settings.item_size);
            let path = PathBuf::from(&entry.path);
            let is_selected = self.selected_entries.contains(&path);
            if entry.is_directory {
                column.push(
                    mouse_area(
                        button(
                            row![
                                icon,
                                name_label(&entry.name, 80, max_name_lines, 20.0, name_alignment,)
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Top),
                        )
                        .style(move |theme, status| {
                            file_item_button_style(theme, status, is_selected)
                        })
                        .width(Length::Fill)
                        .on_press(Message::EntryClicked {
                            path: path.clone(),
                            is_directory: true,
                        }),
                    )
                    .on_right_press(Message::ShowEntryContext {
                        path: path.clone(),
                        is_directory: true,
                    })
                    .interaction(mouse::Interaction::Pointer),
                )
            } else {
                column.push(
                    mouse_area(
                        button(
                            row![
                                icon,
                                name_label(&entry.name, 80, max_name_lines, 20.0, name_alignment,)
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Top),
                        )
                        .style(move |theme, status| {
                            file_item_button_style(theme, status, is_selected)
                        })
                        .width(Length::Fill)
                        .on_press(Message::EntryClicked {
                            path: path.clone(),
                            is_directory: false,
                        }),
                    )
                    .on_right_press(Message::ShowEntryContext {
                        path,
                        is_directory: false,
                    }),
                )
            }
        });
        let entries: Element<'_, Message> = if browser_settings.layout == BrowserLayout::Tiles {
            let tile_columns = Rc::clone(&self.tile_columns);
            responsive(move |size| {
                let tile_width = f32::from(browser_settings.item_size) * 3.5;
                let tile_icon_size = browser_settings.item_size.saturating_mul(9) / 5;
                let tile_name_line_characters = (tile_width / 8.0).floor().max(8.0) as usize;
                let columns = (size.width / tile_width).floor().max(1.0) as usize;
                tile_columns.set(columns);
                let tiles =
                    visible_entries
                        .chunks(columns)
                        .fold(column![].spacing(8), |column, chunk| {
                            let max_tile_name_lines = chunk
                                .iter()
                                .map(|entry| {
                                    name_label_lines(
                                        &entry.name,
                                        tile_name_line_characters,
                                        max_name_lines,
                                    )
                                    .len()
                                })
                                .max()
                                .unwrap_or(1);
                            let tile_height = f32::from(tile_icon_size)
                                + 6.0
                                + 20.0 * max_tile_name_lines as f32
                                + 10.0;
                            let tiles = chunk.iter().fold(row![].spacing(8), |row, entry| {
                                let path = PathBuf::from(&entry.path);
                                let is_selected = self.selected_entries.contains(&path);
                                let icon = self.entry_icon(entry, tile_icon_size);
                                let tile_content = container(
                                    column![
                                        icon,
                                        name_label(
                                            &entry.name,
                                            tile_name_line_characters,
                                            max_name_lines,
                                            20.0,
                                            name_alignment,
                                        )
                                    ]
                                    .spacing(6)
                                    .align_x(iced::alignment::Horizontal::Center),
                                )
                                .width(Length::Fill)
                                .height(Length::Fill)
                                .center_x(Length::Fill)
                                .align_y(iced::alignment::Vertical::Top);
                                let tile = button(tile_content)
                                    .style(move |theme, status| {
                                        file_item_button_style(theme, status, is_selected)
                                    })
                                    .width(Length::Fixed(tile_width))
                                    .height(Length::Fixed(tile_height))
                                    .on_press(Message::EntryClicked {
                                        path: path.clone(),
                                        is_directory: entry.is_directory,
                                    });
                                row.push(
                                    mouse_area(tile)
                                        .on_right_press(Message::ShowEntryContext {
                                            path,
                                            is_directory: entry.is_directory,
                                        })
                                        .interaction(mouse::Interaction::Pointer),
                                )
                            });
                            column.push(tiles)
                        });

                scrollable(tiles)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .direction(modern_vertical_scrollbar())
                    .smooth_scrolling(browser_settings.smooth_scrolling)
                    .scroll_step(f32::from(browser_settings.scroll_step))
                    .style(modern_scrollable_style)
                    .into()
            })
            .into()
        } else {
            entries.into()
        };

        let can_go_back = self.history_index.is_some_and(|index| index > 0);
        let can_go_forward = self
            .history_index
            .is_some_and(|index| index + 1 < self.history.len());
        let mut address_bar = row![
            tooltip(
                button(icon_text("arrow-left"))
                    .on_press_maybe(can_go_back.then_some(Message::NavigateBack)),
                text("Back"),
                tooltip::Position::Bottom,
            ),
            tooltip(
                button(icon_text("arrow-right"))
                    .on_press_maybe(can_go_forward.then_some(Message::NavigateForward)),
                text("Forward"),
                tooltip::Position::Bottom,
            ),
            tooltip(
                button(icon_text("folder-up")).on_press(Message::OpenParent),
                text("Parent folder"),
                tooltip::Position::Bottom,
            ),
            self.address_control(),
        ]
        .spacing(8);
        if self.editing_address {
            address_bar = address_bar.push(tooltip(
                button(icon_text("folder-open")).on_press(Message::OpenAddress),
                text(String::from("Open path")),
                tooltip::Position::Bottom,
            ));
        }
        let has_selection = !self.selected_entries.is_empty();
        let mut quick_toolbar_actions = row![].spacing(8);
        let mut rendered_quick_toolbar_items = HashSet::new();
        for item in &browser_settings.quick_toolbar_items {
            if !rendered_quick_toolbar_items.insert(*item) {
                continue;
            }
            let action: Element<'_, Message> = match item {
                QuickToolbarItem::Refresh => tooltip(
                    button(icon_text("rotate-cw")).on_press(Message::RefreshDirectory),
                    text("Refresh folder"),
                    tooltip::Position::Bottom,
                )
                .into(),
                QuickToolbarItem::CloneWindow => tooltip(
                    button(icon_text("copy")).on_press(Message::CloneWindow),
                    text("Open new window"),
                    tooltip::Position::Bottom,
                )
                .into(),
                QuickToolbarItem::PerformanceDebugger => tooltip(
                    button(icon_text("chart-no-axes-combined"))
                        .on_press(Message::TogglePerformanceDebugger),
                    text("Performance debugger"),
                    tooltip::Position::Bottom,
                )
                .into(),
                QuickToolbarItem::ToggleHiddenFiles => tooltip(
                    button(icon_text(if browser_settings.show_hidden_files {
                        "eye-off"
                    } else {
                        "eye"
                    }))
                    .style(move |theme, status| {
                        if browser_settings.show_hidden_files {
                            button_style::primary(theme, status)
                        } else {
                            button_style::text(theme, status)
                        }
                    })
                    .on_press(Message::ToggleHiddenFiles),
                    text(if browser_settings.show_hidden_files {
                        "Hide hidden files"
                    } else {
                        "Show hidden files"
                    }),
                    tooltip::Position::Bottom,
                )
                .into(),
                QuickToolbarItem::Sort => pick_list(
                    EntrySortOrder::ALL,
                    Some(browser_settings.sort_order),
                    Message::SortOrderSelected,
                )
                .width(Length::Fixed(150.0))
                .style(rounded_pick_list_style)
                .menu_style(rounded_pick_list_menu_style)
                .into(),
                QuickToolbarItem::FolderSort => pick_list(
                    FolderSortSelection::ALL,
                    Some(FolderSortSelection::from(
                        self.folder_sort_override(&self.directory_path),
                    )),
                    Message::FolderSortOverrideSelected,
                )
                .width(Length::Fixed(180.0))
                .style(rounded_pick_list_style)
                .menu_style(rounded_pick_list_menu_style)
                .into(),
                QuickToolbarItem::CompressSelection => tooltip(
                    button(icon_text("archive")).on_press_maybe(has_selection.then_some(
                        Message::ExecuteBrowserCommand(BrowserCommand::CompressSelection),
                    )),
                    text("Compress selection"),
                    tooltip::Position::Bottom,
                )
                .into(),
                QuickToolbarItem::ExtractSelection => tooltip(
                    button(icon_text("archive-restore")).on_press_maybe(has_selection.then_some(
                        Message::ExecuteBrowserCommand(BrowserCommand::ExtractSelection),
                    )),
                    text("Extract selected ZIP archives"),
                    tooltip::Position::Bottom,
                )
                .into(),
            };
            quick_toolbar_actions = quick_toolbar_actions.push(action);
        }
        let quick_actions = container(quick_toolbar_actions)
            .width(Length::Fill)
            .padding([3, 6])
            .style(|_| {
                iced::widget::container::Style::default()
                    .background(Color::from_rgba8(128, 128, 128, 0.12))
                    .border(Border {
                        radius: border_radius().into(),
                        ..Border::default()
                    })
            });
        let save_name_input = self.save_file_name.as_ref().map(|name| {
            let reset = self
                .original_save_file_name
                .as_ref()
                .is_some_and(|original| original != name)
                .then(|| {
                    tooltip(
                        button(icon_text("rotate-ccw")).on_press(Message::ResetSaveFileName),
                        text("Reset file name"),
                        tooltip::Position::Top,
                    )
                });
            container(
                row![
                    text_input("File name", name)
                        .on_input(Message::SaveFileNameChanged)
                        .width(Length::Fill),
                ]
                .push_maybe(reset)
                .spacing(8),
            )
            .padding([6, 0])
            .width(Length::Fill)
        });
        let picker_actions = self.picker.as_ref().map(|picker| {
            let selection_label = match (picker.kind, picker.multiple) {
                (PickerKind::File, false) => "Select file",
                (PickerKind::File, true) => "Select files",
                (PickerKind::Folder, false) => "Select folder",
                (PickerKind::Folder, true) => "Select folders",
            };
            container(
                row![
                    button(text("Cancel")).on_press(Message::CancelPicker),
                    Space::with_width(Length::Fill),
                    button(text(selection_label)).on_press(Message::ConfirmPicker),
                ]
                .spacing(8),
            )
            .padding([6, 0])
            .width(Length::Fill)
        });
        address_bar = address_bar.push(tooltip(
            button(icon_text("settings")).on_press(Message::ShowPreferences),
            text(String::from("Preferences")),
            tooltip::Position::Bottom,
        ));
        let tiles_layout = browser_settings.layout == BrowserLayout::Tiles;
        let info_status = self.status.clone();
        let browser: Element<'_, Message> = if browser_settings.preview_enabled {
            let file_pane: Element<'_, Message> = if tiles_layout {
                container(entries)
                    .width(Length::FillPortion(1))
                    .height(Length::Fill)
                    .into()
            } else {
                scrollable(entries)
                    .width(Length::FillPortion(1))
                    .direction(modern_vertical_scrollbar())
                    .smooth_scrolling(browser_settings.smooth_scrolling)
                    .scroll_step(f32::from(browser_settings.scroll_step))
                    .style(modern_scrollable_style)
                    .into()
            };
            row![
                file_pane,
                scrollable(text(&self.content))
                    .width(Length::FillPortion(2))
                    .direction(modern_vertical_scrollbar())
                    .smooth_scrolling(browser_settings.smooth_scrolling)
                    .scroll_step(f32::from(browser_settings.scroll_step))
                    .style(modern_scrollable_style),
            ]
            .spacing(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else if tiles_layout {
            entries
        } else {
            scrollable(entries)
                .width(Length::Fill)
                .height(Length::Fill)
                .direction(modern_vertical_scrollbar())
                .smooth_scrolling(browser_settings.smooth_scrolling)
                .scroll_step(f32::from(browser_settings.scroll_step))
                .style(modern_scrollable_style)
                .into()
        };
        let selection_overlay: Option<Element<'_, Message>> =
            self.rectangle_selection.as_ref().map(|selection| {
                let left = selection.start.x.min(selection.end.x);
                let top = selection.start.y.min(selection.end.y);
                let width = (selection.start.x - selection.end.x).abs().max(1.0);
                let height = (selection.start.y - selection.end.y).abs().max(1.0);
                container(column![
                    Space::with_height(top),
                    row![
                        Space::with_width(left),
                        container(Space::new(Length::Fixed(width), Length::Fixed(height))).style(
                            |theme: &Theme| {
                                iced::widget::container::Style::default()
                                    .background(Color::from_rgba8(90, 130, 200, 0.22))
                                    .border(Border {
                                        color: theme.palette().primary,
                                        width: 1.0,
                                        radius: border_radius().into(),
                                    })
                            }
                        ),
                    ],
                ])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            });
        let browser: Element<'_, Message> = stack![browser]
            .push_maybe(selection_overlay)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        let browser_press = if self.editing_address {
            Message::CancelAddressEdit
        } else {
            Message::StartRectangleSelection
        };
        let browser = mouse_area(browser)
            .on_right_press(Message::ShowEntryContext {
                path: self.directory_path.clone(),
                is_directory: true,
            })
            .on_press(browser_press)
            .on_move(Message::RectanglePointerMoved)
            .on_release(Message::FinishRectangleSelection);
        let info_overlay: Element<'_, Message> = container(
            container(text(info_status))
                .padding([4, 8])
                .style(|theme: &Theme| {
                    iced::widget::container::Style::default()
                        .background(Color::from_rgba8(128, 128, 128, 0.22))
                        .border(Border {
                            color: theme.palette().primary.scale_alpha(0.45),
                            width: 1.0,
                            radius: border_radius().into(),
                        })
                }),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(8)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Bottom)
        .into();
        let browser: Element<'_, Message> = stack![browser]
            .push(info_overlay)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        let resize_handle = mouse_area(
            row![
                Space::with_width(Length::Fixed(12.0)),
                container(Space::new(Length::Fixed(2.0), Length::Fill))
                    .width(Length::Fixed(2.0))
                    .height(Length::Fill)
                    .style(|_| {
                        iced::widget::container::Style::default()
                            .background(Color::from_rgba8(128, 128, 128, 0.25))
                    }),
            ]
            .width(Length::Fixed(14.0))
            .height(Length::Fill),
        )
        .on_press(Message::StartSidebarResize)
        .on_release(Message::FinishSidebarResize)
        .interaction(mouse::Interaction::ResizingHorizontally);
        let sidebar_panel = row![self.sidebar_view(), resize_handle]
            .spacing(0)
            .height(Length::Fill);
        let file_content = column![quick_actions, browser]
            .push_maybe(save_name_input)
            .push_maybe(picker_actions)
            .spacing(8)
            .width(Length::Fill)
            .height(Length::Fill);
        let main_content = row![sidebar_panel, file_content]
            .spacing(16)
            .width(Length::Fill)
            .height(Length::Fill);
        let content = column![address_bar, main_content];

        let page = container(content.spacing(12).padding(16).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill);
        let overlay: Option<Element<'_, Message>> = self.context_entry.as_ref().map(|entry| {
            responsive(move |size| {
                const CONTEXT_MENU_WIDTH: f32 = 240.0;
                let context_x = if self.context_position.x + CONTEXT_MENU_WIDTH > size.width {
                    (self.context_position.x - CONTEXT_MENU_WIDTH).max(0.0)
                } else {
                    self.context_position.x
                };
                let is_in_sidebar = self
                    .active_sidebar_locations()
                    .iter()
                    .any(|location| location.path == entry.path);
                let mut actions = column![].spacing(4);
                let mut action_count = 0_u16;
                let mut rendered_items = HashSet::new();
                let context_menu_items: &[ContextMenuItem] = if entry.is_sidebar_location {
                    &[]
                } else if entry.is_directory {
                    &browser_settings.folder_context_menu_items
                } else {
                    &browser_settings.file_context_menu_items
                };
                for item in context_menu_items {
                    if !rendered_items.insert(*item) {
                        continue;
                    }
                    let action: Option<Element<'_, Message>> = match item {
                        ContextMenuItem::Info => Some(
                            button(row![icon_text("info").size(16), text("Info")].spacing(8))
                                .width(Length::Fill)
                                .style(context_menu_button_style)
                                .on_press(Message::RequestEntryInfo(entry.path.clone()))
                                .into(),
                        ),
                        ContextMenuItem::CreateFolder if entry.is_directory => Some(
                            button(
                                row![icon_text("folder-plus").size(16), text("Create folder")]
                                    .spacing(8),
                            )
                            .width(Length::Fill)
                            .style(context_menu_button_style)
                            .on_press(Message::RequestCreateEntry {
                                parent: entry.path.clone(),
                                is_directory: true,
                            })
                            .into(),
                        ),
                        ContextMenuItem::CreateFile if entry.is_directory => Some(
                            button(
                                row![icon_text("file-plus").size(16), text("Create file")]
                                    .spacing(8),
                            )
                            .width(Length::Fill)
                            .style(context_menu_button_style)
                            .on_press(Message::RequestCreateEntry {
                                parent: entry.path.clone(),
                                is_directory: false,
                            })
                            .into(),
                        ),
                        ContextMenuItem::Rename => Some(
                            button(row![icon_text("pencil").size(16), text("Rename")].spacing(8))
                                .width(Length::Fill)
                                .style(context_menu_button_style)
                                .on_press(Message::RequestRenameEntry(entry.path.clone()))
                                .into(),
                        ),
                        ContextMenuItem::Duplicate => Some(
                            button(row![icon_text("copy").size(16), text("Duplicate")].spacing(8))
                                .width(Length::Fill)
                                .style(context_menu_button_style)
                                .on_press(Message::DuplicateContextEntry(entry.path.clone()))
                                .into(),
                        ),
                        ContextMenuItem::Open if !entry.is_directory => Some(
                            button(
                                row![
                                    icon_text("external-link").size(16),
                                    text(
                                        entry
                                            .opener
                                            .as_ref()
                                            .and_then(|opener| opener.as_ref().ok())
                                            .map(|application| format!("Open (with {application})"))
                                            .unwrap_or_else(|| "Open".into()),
                                    )
                                ]
                                .spacing(8),
                            )
                            .width(Length::Fill)
                            .style(context_menu_button_style)
                            .on_press_maybe(
                                entry.opener.is_some().then_some(Message::OpenContextFile),
                            )
                            .into(),
                        ),
                        ContextMenuItem::CopyLocation => Some(
                            button(
                                row![icon_text("copy").size(16), text("Copy location")].spacing(8),
                            )
                            .width(Length::Fill)
                            .style(context_menu_button_style)
                            .on_press(Message::ExecuteBrowserCommand(
                                BrowserCommand::CopyLocation(entry.path.clone()),
                            ))
                            .into(),
                        ),
                        ContextMenuItem::CopySelection if !self.selected_entries.is_empty() => {
                            Some(
                                button(
                                    row![icon_text("copy").size(16), text("Copy selection")]
                                        .spacing(8),
                                )
                                .width(Length::Fill)
                                .style(context_menu_button_style)
                                .on_press(Message::ExecuteBrowserCommand(
                                    BrowserCommand::CopySelection,
                                ))
                                .into(),
                            )
                        }
                        ContextMenuItem::DeleteSelection if !self.selected_entries.is_empty() => {
                            Some(
                                button(
                                    row![icon_text("trash-2").size(16), text("Delete selection")]
                                        .spacing(8),
                                )
                                .width(Length::Fill)
                                .style(context_menu_button_style)
                                .on_press(Message::ExecuteBrowserCommand(
                                    BrowserCommand::DeleteSelection,
                                ))
                                .into(),
                            )
                        }
                        ContextMenuItem::Paste if self.paste_buffer.is_some() => Some(
                            button(
                                row![icon_text("clipboard-paste").size(16), text("Paste")]
                                    .spacing(8),
                            )
                            .width(Length::Fill)
                            .style(context_menu_button_style)
                            .on_press(Message::ExecuteBrowserCommand(BrowserCommand::Paste))
                            .into(),
                        ),
                        ContextMenuItem::ToggleSidebarLocation if entry.is_directory => {
                            Some(if is_in_sidebar {
                                button(
                                    row![
                                        icon_text("folder-minus").size(16),
                                        text("Remove from sidebar")
                                    ]
                                    .spacing(8),
                                )
                                .width(Length::Fill)
                                .style(context_menu_button_style)
                                .on_press(Message::RemoveContextFolderFromSidebar)
                                .into()
                            } else {
                                button(
                                    row![icon_text("folder-plus").size(16), text("Add to sidebar")]
                                        .spacing(8),
                                )
                                .width(Length::Fill)
                                .style(context_menu_button_style)
                                .on_press(Message::AddContextFolderToSidebar)
                                .into()
                            })
                        }
                        ContextMenuItem::CreateSymlink if entry.is_directory => Some(
                            button(
                                row![icon_text("link").size(16), text("Create symlink here")]
                                    .spacing(8),
                            )
                            .width(Length::Fill)
                            .style(context_menu_button_style)
                            .on_press(Message::ExecuteBrowserCommand(
                                BrowserCommand::CreateSymlinksHere(entry.path.clone()),
                            ))
                            .into(),
                        ),
                        ContextMenuItem::AddSymlinkToPasteBuffer if entry.is_directory => Some(
                            button(
                                row![
                                    icon_text("link").size(16),
                                    text("Add symlink to paste buffer")
                                ]
                                .spacing(8),
                            )
                            .width(Length::Fill)
                            .style(context_menu_button_style)
                            .on_press(Message::ExecuteBrowserCommand(
                                BrowserCommand::AddSymlinkToPasteBuffer(entry.path.clone()),
                            ))
                            .into(),
                        ),
                        ContextMenuItem::OpenTerminal if entry.is_directory => Some(
                            button(
                                row![icon_text("terminal").size(16), text("Open terminal here")]
                                    .spacing(8),
                            )
                            .width(Length::Fill)
                            .style(context_menu_button_style)
                            .on_press(Message::OpenTerminalHere)
                            .into(),
                        ),
                        _ => None,
                    };
                    if let Some(action) = action {
                        actions = actions.push(action);
                        action_count += 1;
                    }
                }
                if entry.is_sidebar_location {
                    let icon_picker = [
                        "house",
                        "download",
                        "image",
                        "folder",
                        "star",
                        "file-text",
                        "landmark",
                        "git-branch",
                        "music",
                        "video",
                        "archive",
                        "hard-drive",
                    ]
                    .chunks(3)
                    .fold(column![].spacing(4), |column, icons| {
                        column.push(icons.iter().fold(row![].spacing(4), |row, icon| {
                            row.push(
                                button(icon_text(icon).size(16))
                                    .width(Length::Fill)
                                    .style(context_menu_button_style)
                                    .on_press(Message::SetSidebarLocationIcon {
                                        path: entry.path.clone(),
                                        icon: Some((*icon).to_owned()),
                                    }),
                            )
                        }))
                    });
                    actions = actions
                        .push(text("Icon").size(14))
                        .push(icon_picker)
                        .push(
                            button(
                                row![icon_text("rotate-ccw").size(16), text("Reset icon")]
                                    .spacing(8),
                            )
                            .width(Length::Fill)
                            .style(context_menu_button_style)
                            .on_press(
                                Message::SetSidebarLocationIcon {
                                    path: entry.path.clone(),
                                    icon: None,
                                },
                            ),
                        )
                        .push(
                            button(
                                row![
                                    icon_text("folder-minus").size(16),
                                    text("Remove from sidebar")
                                ]
                                .spacing(8),
                            )
                            .width(Length::Fill)
                            .style(context_menu_button_style)
                            .on_press(Message::RemoveContextFolderFromSidebar),
                        );
                    action_count += 7;
                }
                let theme_settings = self.active_theme_settings();
                let context_menu_blur_strength = if theme_settings.background_opacity < 100 {
                    theme_settings.context_menu_blur_strength.min(5)
                } else {
                    0
                };
                let context_menu_blur_kernel_size = theme_settings
                    .context_menu_blur_kernel_size
                    .effective_size(context_menu_blur_strength);
                let menu = opaque(
                    container(actions)
                        .width(Length::Fixed(CONTEXT_MENU_WIDTH))
                        .padding(8)
                        .style(move |theme: &Theme| {
                            iced::widget::container::Style::default()
                                .background(theme.palette().background)
                                .border(Border {
                                    color: Color::from_rgba8(128, 128, 128, 0.45),
                                    width: 1.0,
                                    radius: border_radius().into(),
                                })
                                .shadow(Shadow {
                                    color: Color::BLACK.scale_alpha(
                                        (context_menu_blur_strength > 0)
                                            .then_some(0.35)
                                            .unwrap_or(0.0),
                                    ),
                                    offset: Vector::new(0.0, 3.0),
                                    blur_radius: f32::from(context_menu_blur_strength),
                                })
                        }),
                );
                let menu_height = f32::from(action_count) * 30.0
                    + f32::from(action_count.saturating_sub(1)) * 4.0
                    + 16.0;
                let context_y = if self.context_position.y + menu_height > size.height {
                    (self.context_position.y - menu_height).max(0.0)
                } else {
                    self.context_position.y
                };
                let menu_position = container(column![
                    Space::with_height(context_y),
                    row![Space::with_width(context_x), menu],
                ])
                .width(Length::Fill)
                .height(Length::Fill);
                let blur_position: Element<'_, Message> = if context_menu_blur_strength > 0 {
                    container(column![
                        Space::with_height(context_y),
                        row![
                            Space::with_width(context_x),
                            iced::widget::shader::Shader::new(backdrop_blur::BackdropBlur::new(
                                context_menu_blur_strength,
                                context_menu_blur_kernel_size,
                            ))
                            .width(Length::Fixed(CONTEXT_MENU_WIDTH))
                            .height(Length::Fixed(menu_height)),
                        ],
                    ])
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
                } else {
                    Space::new(Length::Fill, Length::Fill).into()
                };
                stack![
                    mouse_area(Space::new(Length::Fill, Length::Fill))
                        .on_press(Message::CloseFolderContext)
                        .on_right_press(Message::CloseFolderContext),
                    blur_position,
                    menu_position,
                ]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            })
            .into()
        });

        let page: Element<'_, Message> = if self.editing_address {
            stack![
                mouse_area(Space::new(Length::Fill, Length::Fill))
                    .on_press(Message::CancelAddressEdit)
                    .on_move(Message::ContextPointerMoved)
                    .on_release(Message::FinishSidebarResize),
                page,
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            mouse_area(page)
                .on_move(Message::ContextPointerMoved)
                .on_release(Message::FinishSidebarResize)
                .into()
        };
        let delete_confirmation = self.pending_delete.as_ref().map(|paths| {
            let delete_confirm_selected = self.delete_confirm_selected;
            let dialog = container(
                column![
                    text(format!("Delete {} item(s)?", paths.len())),
                    text("This permanently deletes the selected files and folders."),
                    row![
                        button(text("Cancel"))
                            .style(move |theme, status| {
                                if !delete_confirm_selected {
                                    button_style::primary(theme, status)
                                } else {
                                    button_style::secondary(theme, status)
                                }
                            })
                            .on_press(Message::CancelDelete),
                        button(text("Delete"))
                            .style(move |theme, status| {
                                if delete_confirm_selected {
                                    button_style::primary(theme, status)
                                } else {
                                    button_style::secondary(theme, status)
                                }
                            })
                            .on_press(Message::ConfirmDelete),
                    ]
                    .spacing(8),
                ]
                .spacing(12),
            )
            .padding(16)
            .style(|theme: &Theme| {
                iced::widget::container::Style::default()
                    .background(theme.palette().background)
                    .border(Border {
                        color: theme.palette().primary,
                        width: 1.0,
                        radius: border_radius().into(),
                    })
            });
            stack![
                mouse_area(Space::new(Length::Fill, Length::Fill)).on_press(Message::CancelDelete),
                container(dialog)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
        });
        let create_dialog = self.pending_create.as_ref().map(|(_, is_directory)| {
            let kind = if *is_directory { "folder" } else { "file" };
            let dialog = container(
                column![
                    text(format!("Create {kind}")),
                    text_input("Name", &self.create_entry_name)
                        .on_input(Message::CreateEntryNameChanged)
                        .on_submit(Message::ConfirmCreateEntry),
                    row![
                        button(text("Cancel")).on_press(Message::CancelCreateEntry),
                        button(text("Create")).on_press(Message::ConfirmCreateEntry),
                    ]
                    .spacing(8),
                ]
                .spacing(12),
            )
            .padding(16)
            .style(|theme: &Theme| {
                iced::widget::container::Style::default()
                    .background(theme.palette().background)
                    .border(Border {
                        color: theme.palette().primary,
                        width: 1.0,
                        radius: border_radius().into(),
                    })
            });
            stack![
                mouse_area(Space::new(Length::Fill, Length::Fill))
                    .on_press(Message::CancelCreateEntry),
                container(dialog)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
        });
        let rename_dialog = self.pending_rename.as_ref().map(|_| {
            let dialog = container(
                column![
                    text("Rename"),
                    text_input("Name", &self.rename_entry_name)
                        .on_input(Message::RenameEntryNameChanged)
                        .on_submit(Message::ConfirmRenameEntry),
                    row![
                        button(text("Cancel")).on_press(Message::CancelRenameEntry),
                        button(text("Rename")).on_press(Message::ConfirmRenameEntry),
                    ]
                    .spacing(8),
                ]
                .spacing(12),
            )
            .padding(16)
            .style(|theme: &Theme| {
                iced::widget::container::Style::default()
                    .background(theme.palette().background)
                    .border(Border {
                        color: theme.palette().primary,
                        width: 1.0,
                        radius: border_radius().into(),
                    })
            });
            stack![
                mouse_area(Space::new(Length::Fill, Length::Fill))
                    .on_press(Message::CancelRenameEntry),
                container(dialog)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
        });
        let compression_dialog = self.pending_compression.then(|| {
            let dialog = container(
                column![
                    text("Compress selection"),
                    text("Compression type"),
                    radio(
                        "Store (no compression)",
                        ArchiveCompression::Store,
                        Some(self.compression_type),
                        Message::CompressionTypeSelected,
                    ),
                    radio(
                        "Deflate",
                        ArchiveCompression::Deflate,
                        Some(self.compression_type),
                        Message::CompressionTypeSelected,
                    ),
                    radio(
                        "Bzip2",
                        ArchiveCompression::Bzip2,
                        Some(self.compression_type),
                        Message::CompressionTypeSelected,
                    ),
                    radio(
                        "Zstd",
                        ArchiveCompression::Zstd,
                        Some(self.compression_type),
                        Message::CompressionTypeSelected,
                    ),
                    row![
                        text("Compression level"),
                        slider(
                            0..=9,
                            self.compression_level,
                            Message::CompressionLevelChanged
                        )
                        .width(Length::Fill),
                        text(self.compression_level.to_string()),
                    ]
                    .spacing(10),
                    row![
                        button(text("Cancel")).on_press(Message::CancelCompression),
                        button(text("Compress")).on_press(Message::ConfirmCompression),
                    ]
                    .spacing(8),
                ]
                .spacing(12),
            )
            .width(Length::Fixed(320.0))
            .padding(16)
            .style(|theme: &Theme| {
                iced::widget::container::Style::default()
                    .background(theme.palette().background)
                    .border(Border {
                        color: theme.palette().primary,
                        width: 1.0,
                        radius: border_radius().into(),
                    })
            });
            stack![
                mouse_area(Space::new(Length::Fill, Length::Fill))
                    .on_press(Message::CancelCompression),
                container(dialog)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
        });
        let info_dialog = self.pending_info.as_ref().map(|dialog_state| {
            const INFO_DIALOG_WIDTH: f32 = 460.0;
            const INFO_DIALOG_MIN_HEIGHT: f32 = 180.0;
            const INFO_DIALOG_MAX_HEIGHT: f32 = 640.0;
            const INFO_DIALOG_CHROME_HEIGHT: f32 = 152.0;
            const INFO_ROW_HEIGHT: f32 = 28.0;
            const INFO_THUMBNAIL_HEIGHT: f32 = 176.0;
            let info_header = || {
                row![
                    text("Info").size(20),
                    Space::with_width(Length::Fill),
                    tooltip(
                        button(icon_text("x")).on_press(Message::CloseEntryInfo),
                        text("Close"),
                        tooltip::Position::Bottom,
                    ),
                ]
                .align_y(iced::alignment::Vertical::Center)
            };
            let info_dialog_height = match dialog_state {
                InfoDialog::Loading(_) => INFO_DIALOG_MIN_HEIGHT,
                InfoDialog::Error { .. } => 220.0,
                InfoDialog::Loaded(info) => (INFO_DIALOG_CHROME_HEIGHT
                    + self
                        .thumbnail_handles
                        .contains_key(&info.path)
                        .then_some(INFO_THUMBNAIL_HEIGHT)
                        .unwrap_or_default()
                    + info.rows.len() as f32 * INFO_ROW_HEIGHT)
                    .clamp(INFO_DIALOG_MIN_HEIGHT, INFO_DIALOG_MAX_HEIGHT),
            };
            let dialog: Element<'_, Message> = match dialog_state {
                InfoDialog::Loading(path) => container(
                    column![
                        info_header(),
                        text(path.display().to_string()),
                        text("Loading details..."),
                    ]
                    .spacing(12),
                )
                .width(Length::Fixed(INFO_DIALOG_WIDTH))
                .padding(16)
                .into(),
                InfoDialog::Loaded(info) => {
                    let rows_height = info.rows.len() as f32 * INFO_ROW_HEIGHT;
                    let thumbnail: Option<Element<'_, Message>> =
                        self.thumbnail_handles.get(&info.path).map(|handle| {
                            container(
                                image(handle.clone())
                                    .width(Length::Fixed(160.0))
                                    .height(Length::Fixed(160.0)),
                            )
                            .width(Length::Fill)
                            .center_x(Length::Fill)
                            .into()
                        });
                    let max_details_height = INFO_DIALOG_MAX_HEIGHT
                        - INFO_DIALOG_CHROME_HEIGHT
                        - thumbnail
                            .as_ref()
                            .map(|_| INFO_THUMBNAIL_HEIGHT)
                            .unwrap_or_default();
                    let rows =
                        info.rows
                            .iter()
                            .fold(column![].spacing(8), |column, (label, value)| {
                                column.push(
                                    row![
                                        text(label).width(Length::Fixed(112.0)),
                                        text(value).width(Length::Fill),
                                    ]
                                    .spacing(12),
                                )
                            });
                    let details: Element<'_, Message> = if rows_height > max_details_height {
                        scrollable(rows)
                            .height(Length::Fixed(max_details_height))
                            .into()
                    } else {
                        rows.into()
                    };
                    let content = column![info_header(), text(&info.name)]
                        .push_maybe(thumbnail)
                        .push(details)
                        .spacing(12);
                    container(content)
                        .width(Length::Fixed(INFO_DIALOG_WIDTH))
                        .padding(16)
                        .into()
                }
                InfoDialog::Error { path, error } => container(
                    column![
                        info_header(),
                        text(path.display().to_string()),
                        text(format!("Unable to read details: {error}")),
                    ]
                    .spacing(12),
                )
                .width(Length::Fixed(INFO_DIALOG_WIDTH))
                .padding(16)
                .into(),
            };
            let theme_settings = self.active_theme_settings();
            let blur_strength = if theme_settings.background_opacity < 100 {
                theme_settings.context_menu_blur_strength.min(5)
            } else {
                0
            };
            let blur_kernel_size = theme_settings
                .context_menu_blur_kernel_size
                .effective_size(blur_strength);
            let dialog = container(dialog)
                .width(Length::Fixed(INFO_DIALOG_WIDTH))
                .height(Length::Fixed(info_dialog_height))
                .style(|theme: &Theme| {
                    iced::widget::container::Style::default()
                        .background(theme.palette().background)
                        .border(Border {
                            color: theme.palette().primary,
                            width: 1.0,
                            radius: border_radius().into(),
                        })
                });
            let blur: Element<'_, Message> = if blur_strength > 0 {
                iced::widget::shader::Shader::new(backdrop_blur::BackdropBlur::new(
                    blur_strength,
                    blur_kernel_size,
                ))
                .width(Length::Fixed(INFO_DIALOG_WIDTH))
                .height(Length::Fixed(info_dialog_height))
                .into()
            } else {
                Space::new(
                    Length::Fixed(INFO_DIALOG_WIDTH),
                    Length::Fixed(info_dialog_height),
                )
                .into()
            };
            let modal = container(stack![blur, dialog])
                .width(Length::Fixed(INFO_DIALOG_WIDTH))
                .height(Length::Fixed(info_dialog_height));
            stack![
                mouse_area(Space::new(Length::Fill, Length::Fill))
                    .on_press(Message::CloseEntryInfo),
                mouse_area(
                    container(modal)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_move(Message::ContextPointerMoved),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
        });
        let performance_debugger = self.show_performance_debugger.then(|| {
            let timeline: Element<'_, Message> = if let Some(load) = &self.folder_load_performance {
                let displayed_ms = load
                    .displayed_at
                    .map(|at| at.duration_since(load.started_at).as_millis() as u64)
                    .unwrap_or_default();
                let thumbnails_ms = load
                    .thumbnails_settled_at
                    .map(|at| at.duration_since(load.started_at).as_millis() as u64)
                    .unwrap_or_default();
                let first_item_ms = load
                    .first_item_at
                    .map(|at| at.duration_since(load.started_at).as_millis() as u64)
                    .unwrap_or_default();
                let response_ms = self
                    .last_folder_load_duration
                    .map(|duration| duration.as_millis() as u64)
                    .unwrap_or_default();
                let total_ms = displayed_ms
                    .max(thumbnails_ms)
                    .max(first_item_ms)
                    .max(response_ms)
                    .max(1);
                let enumeration_width = (420.0 * load.enumeration_ms.unwrap_or_default() as f32
                    / total_ms as f32)
                    .max(3.0);
                let display_width = (420.0 * displayed_ms as f32 / total_ms as f32).max(3.0);
                let thumbnail_width = (420.0 * thumbnails_ms as f32 / total_ms as f32).max(3.0);
                let response_width = (420.0 * response_ms as f32 / total_ms as f32).max(3.0);
                let first_item_width = (420.0 * first_item_ms as f32 / total_ms as f32).max(3.0);
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
                                "{percent}%: {} ms",
                                at.duration_since(load.started_at).as_millis()
                            )
                        })
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                let mut item_intervals = load
                    .item_rendered_at
                    .windows(2)
                    .map(|timestamps| {
                        timestamps[1]
                            .duration_since(timestamps[0])
                            .as_secs_f64()
                            * 1_000.0
                    })
                    .collect::<Vec<_>>();
                item_intervals.sort_by(f64::total_cmp);
                let item_render_stats = if item_intervals.is_empty() {
                    "Item render intervals: waiting for more items".to_owned()
                } else {
                    let average = item_intervals.iter().sum::<f64>() / item_intervals.len() as f64;
                    let percentile = |percent: usize| item_intervals[(item_intervals.len() - 1) * percent / 100];
                    format!(
                        "Item render interval: avg {average:.2} ms | min {:.2} ms | p5 {:.2} ms | p95 {:.2} ms | max {:.2} ms",
                        item_intervals[0],
                        percentile(5),
                        percentile(95),
                        item_intervals[item_intervals.len() - 1],
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
                column![
                    text(format!("{} entries", load.expected_entries)),
                    text(format!("Folder load total: {total_ms} ms")).size(14),
                    text(format!("Browse response received: {response_ms} ms")).size(14),
                    container(Space::with_height(Length::Fixed(14.0)))
                        .width(Length::Fixed(response_width))
                        .style(|theme: &Theme| iced::widget::container::Style::default()
                            .background(theme.palette().primary)),
                    text(format!(
                        "Backend enumeration: {} ms",
                        load.enumeration_ms.unwrap_or_default()
                    ))
                    .size(14),
                    container(Space::with_height(Length::Fixed(20.0)))
                        .width(Length::Fixed(enumeration_width))
                        .style(|theme: &Theme| iced::widget::container::Style::default()
                            .background(theme.palette().danger)),
                    text(format!("First item rendered: {first_item_ms} ms")).size(14),
                    text(format!("Item milestones: {item_milestones}")).size(13),
                    text(item_render_stats).size(13),
                    text(backend_item_averages).size(13),
                    container(Space::with_height(Length::Fixed(14.0)))
                        .width(Length::Fixed(first_item_width))
                        .style(|theme: &Theme| iced::widget::container::Style::default()
                            .background(theme.palette().primary)),
                    text(format!("All items displayed: {displayed_ms} ms")).size(14),
                    container(Space::with_height(Length::Fixed(20.0)))
                        .width(Length::Fixed(display_width))
                        .style(|theme: &Theme| iced::widget::container::Style::default()
                            .background(theme.palette().primary)),
                    text(format!(
                        "All thumbnails loaded: {thumbnails_ms} ms ({}/{})",
                        load.thumbnails_settled, load.thumbnails_total
                    ))
                    .size(14),
                    container(Space::with_height(Length::Fixed(20.0)))
                        .width(Length::Fixed(thumbnail_width))
                        .style(|theme: &Theme| iced::widget::container::Style::default()
                            .background(theme.palette().success)),
                ]
                .spacing(6)
                .into()
            } else {
                text("Loading performance data...").into()
            };
            let dialog = container(
                column![
                    row![
                        text("Last Folder Load").size(20),
                        Space::with_width(Length::Fill),
                        button(icon_text("copy")).on_press(Message::CopyPerformanceReport),
                        button(icon_text("x")).on_press(Message::TogglePerformanceDebugger),
                    ]
                    .align_y(iced::alignment::Vertical::Center),
                    text(self.directory_path.display().to_string()),
                    container(scrollable(timeline).direction(modern_vertical_scrollbar()))
                        .max_height(520.0),
                ]
                .spacing(12),
            )
            .width(Length::Fixed(520.0))
            .padding(16)
            .style(|theme: &Theme| {
                iced::widget::container::Style::default()
                    .background(theme.palette().background)
                    .border(Border {
                        color: theme.palette().primary,
                        width: 1.0,
                        radius: border_radius().into(),
                    })
            });
            let theme_settings = self.active_theme_settings();
            let blur_strength = if theme_settings.background_opacity < 100 {
                theme_settings.context_menu_blur_strength.min(5)
            } else {
                0
            };
            let blur: Element<'_, Message> = if blur_strength > 0 {
                iced::widget::shader::Shader::new(backdrop_blur::BackdropBlur::new(
                    blur_strength,
                    theme_settings
                        .context_menu_blur_kernel_size
                        .effective_size(blur_strength),
                ))
                .width(Length::Fixed(520.0))
                .height(Length::Fixed(640.0))
                .into()
            } else {
                Space::new(Length::Fixed(520.0), Length::Fixed(360.0)).into()
            };
            let modal = container(stack![blur, dialog])
                .width(Length::Fixed(520.0))
                .max_height(640.0);
            stack![
                mouse_area(Space::new(Length::Fill, Length::Fill))
                    .on_press(Message::TogglePerformanceDebugger)
                    .on_move(Message::ContextPointerMoved),
                container(modal)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
        });

        stack![page]
            .push_maybe(overlay)
            .push_maybe(delete_confirmation)
            .push_maybe(create_dialog)
            .push_maybe(rename_dialog)
            .push_maybe(compression_dialog)
            .push_maybe(info_dialog)
            .push_maybe(performance_debugger)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn entry_icon<'a>(&self, entry: &proto::FileEntry, size: u16) -> Element<'a, Message> {
        let opacity = entry.name.starts_with('.').then_some(0.55).unwrap_or(1.0);
        let icon: Element<'a, Message> =
            if let Some(handle) = self.thumbnail_handles.get(&PathBuf::from(&entry.path)) {
                image(handle.clone())
                    .width(Length::Fixed(f32::from(size)))
                    .height(Length::Fixed(f32::from(size)))
                    .opacity(opacity)
                    .into()
            } else if let Some(path) = self
                .entry_icons
                .get(&PathBuf::from(&entry.path))
                .and_then(|path| path.as_ref())
            {
                svg(svg::Handle::from_path(path))
                    .width(Length::Fixed(f32::from(size)))
                    .height(Length::Fixed(f32::from(size)))
                    .opacity(opacity)
                    .into()
            } else {
                let icon = icon_text(if entry.is_directory { "folder" } else { "file" }).size(size);
                if entry.name.starts_with('.') {
                    icon.color(Color::from_rgba8(128, 128, 128, opacity)).into()
                } else {
                    icon.into()
                }
            };
        if !entry.is_symlink {
            return icon;
        }

        let badge_edge = (f32::from(size) * 0.24).clamp(10.0, 18.0);
        let badge = container(
            icon_text("link")
                .size((badge_edge - 4.0).max(7.0) as u16)
                .color(Color::from_rgb8(35, 35, 35)),
        )
        .width(Length::Fixed(badge_edge))
        .height(Length::Fixed(badge_edge))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(|_| {
            iced::widget::container::Style::default()
                .background(Color::from_rgba8(235, 235, 235, 0.92))
                .border(Border {
                    color: Color::from_rgba8(70, 70, 70, 0.7),
                    width: 1.0,
                    radius: border_radius().into(),
                })
        });
        stack![
            container(icon)
                .width(Length::Fixed(f32::from(size)))
                .height(Length::Fixed(f32::from(size))),
            container(badge)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::alignment::Horizontal::Right)
                .align_y(iced::alignment::Vertical::Bottom),
        ]
        .width(Length::Fixed(f32::from(size)))
        .height(Length::Fixed(f32::from(size)))
        .into()
    }

    fn address_control(&self) -> Element<'_, Message> {
        if self.editing_address {
            return container(
                text_input("Path", &self.address)
                    .on_input(Message::AddressChanged)
                    .on_submit(Message::OpenAddress)
                    .padding([6, 5])
                    .style(rounded_text_input_style)
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fixed(NAVIGATION_CONTROL_HEIGHT))
            .align_y(iced::alignment::Vertical::Center)
            .into();
        }

        let path = PathBuf::from(&self.address);
        let mut target = PathBuf::new();
        let mut breadcrumbs = row![].spacing(2);
        for component in path.components() {
            use std::path::Component;

            let label = match component {
                Component::RootDir => {
                    target.push(component.as_os_str());
                    "/".into()
                }
                Component::CurDir => {
                    target.push(component.as_os_str());
                    ".".into()
                }
                Component::ParentDir => {
                    target.push(component.as_os_str());
                    "..".into()
                }
                Component::Normal(name) => {
                    target.push(name);
                    name.to_string_lossy().into_owned()
                }
                Component::Prefix(prefix) => {
                    target.push(prefix.as_os_str());
                    prefix.as_os_str().to_string_lossy().into_owned()
                }
            };
            breadcrumbs = breadcrumbs.push(
                button(text(label))
                    .style(rounded_text_button_style)
                    .on_press(Message::OpenPath(target.clone())),
            );
        }

        container(
            stack![
                mouse_area(Space::new(
                    Length::Fill,
                    Length::Fixed(NAVIGATION_CONTROL_HEIGHT),
                ))
                .on_press(Message::StartAddressEdit),
                breadcrumbs,
            ]
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .padding([0, 6])
        .width(Length::Fill)
        .height(Length::Fixed(NAVIGATION_CONTROL_HEIGHT))
        .clip(true)
        .align_y(iced::alignment::Vertical::Center)
        .style(|theme: &Theme| {
            iced::widget::container::Style::default().border(Border {
                color: theme.extended_palette().background.strong.color,
                width: 1.0,
                radius: border_radius().into(),
            })
        })
        .into()
    }

    fn sidebar_view(&self) -> Element<'_, Message> {
        let browser_settings = self.active_browser_settings();
        let locations = self.active_sidebar_locations().into_iter().fold(
            column![].spacing(6),
            |column, location| {
                let is_dragging = self.dragging_sidebar_location.as_ref() == Some(&location.path);
                let is_open = self.directory_path == location.path;
                let is_drop_target = self.dragging_sidebar_location.is_some()
                    && !is_dragging
                    && self.sidebar_drop_target.as_ref() == Some(&location.path);
                let label = location.label.clone();
                let item = container(
                    row![
                        icon_text(sidebar_icon(&location)),
                        text(truncate_label(&label, 24))
                            .width(Length::Fill)
                            .height(Length::Fixed(20.0))
                            .wrapping(Wrapping::None)
                    ]
                    .spacing(8)
                    .align_y(iced::alignment::Vertical::Top),
                )
                .padding(8)
                .width(Length::Fill);
                let item: Element<'_, Message> = stack![
                    button(item)
                        .padding(0)
                        .width(Length::Fill)
                        .style(move |theme, status| {
                            file_item_button_style(theme, status, is_open)
                        })
                        .on_press(Message::SidebarPressed(location.path.clone())),
                    mouse_area(Space::new(Length::Fill, Length::Fill))
                        .on_press(Message::SidebarPressed(location.path.clone()))
                        .on_release(Message::SidebarReleased(location.path.clone()))
                        .on_right_press(Message::ShowSidebarLocationContext(location.path.clone()))
                        .on_enter(Message::SidebarDragTarget(location.path.clone()))
                        .on_exit(Message::SidebarDragTargetCleared(location.path.clone())),
                ]
                .width(Length::Fill)
                .into();
                let item: Element<'_, Message> = if is_drop_target {
                    stack![
                        item,
                        container(Space::with_height(Length::Fixed(2.0)))
                            .width(Length::Fill)
                            .height(Length::Fixed(2.0))
                            .style(|theme: &Theme| {
                                iced::widget::container::Style::default()
                                    .background(theme.palette().primary)
                            }),
                    ]
                    .width(Length::Fill)
                    .into()
                } else {
                    item.into()
                };
                column.push(item)
            },
        );
        let mounted = self.mounts.iter().fold(
            column![text("Mounted").size(16)].spacing(4),
            |column, mount| {
                column.push(
                    button(
                        container(
                            row![
                                icon_text("hard-drive"),
                                text(mount.path.display().to_string()),
                            ]
                            .spacing(8),
                        )
                        .padding(8)
                        .width(Length::Fill),
                    )
                    .padding(0)
                    .style(|theme, status| file_item_button_style(theme, status, false))
                    .width(Length::Fill)
                    .on_press(Message::OpenPath(mount.path.clone())),
                )
            },
        );
        let unmounted = self
            .drives
            .iter()
            .filter(|drive| drive.mount_points.is_empty())
            .fold(
                column![text("Available").size(16)].spacing(4),
                |column, drive| {
                    column.push(
                        button(
                            container(
                                row![
                                    container(
                                        row![icon_text("hard-drive"), text(&drive.name)].spacing(8)
                                    )
                                    .width(Length::Fill),
                                    icon_text("play").size(16),
                                ]
                                .spacing(8),
                            )
                            .padding(8)
                            .width(Length::Fill),
                        )
                        .padding(0)
                        .style(|theme, status| file_item_button_style(theme, status, false))
                        .width(Length::Fill)
                        .on_press(Message::MountDrive(drive.path.clone())),
                    )
                },
            );
        let mounts = column![mounted, unmounted].spacing(6);
        let drop_zone_height = 4.0;
        let end_drop_zone = mouse_area(
            container(Space::with_height(Length::Fixed(drop_zone_height)))
                .width(Length::Fill)
                .height(Length::Fixed(drop_zone_height))
                .style(move |theme: &Theme| {
                    if self.sidebar_drop_at_end {
                        iced::widget::container::Style::default()
                            .background(theme.palette().primary)
                    } else {
                        iced::widget::container::Style::default()
                    }
                }),
        )
        .on_release(Message::SidebarReleasedAtEnd)
        .on_enter(Message::SidebarDragTargetEnd)
        .on_exit(Message::SidebarDragTargetEndCleared);
        let sidebar_content = column![locations, end_drop_zone, mounts].spacing(20);
        container(
            scrollable(sidebar_content)
                .direction(modern_vertical_scrollbar())
                .smooth_scrolling(browser_settings.smooth_scrolling)
                .scroll_step(f32::from(browser_settings.scroll_step))
                .style(modern_scrollable_style),
        )
        .width(Length::Fixed(f32::from(self.sidebar_width())))
        .height(Length::Fill)
        .into()
    }
}
