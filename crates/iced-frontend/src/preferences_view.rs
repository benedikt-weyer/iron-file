use super::*;
use iced::widget::{column, row, stack};

impl Gui {
    pub(super) fn preferences_view(&self) -> Element<'_, Message> {
        let back_button = tooltip(
            button(icon_text("arrow-left")).on_press(Message::ShowBrowser),
            text("Back to files"),
            tooltip::Position::Bottom,
        );
        let options = column![
            radio(
                "Day",
                ColorMode::Day,
                Some(self.color_mode),
                Message::ColorModeSelected,
            ),
            radio(
                "Night",
                ColorMode::Night,
                Some(self.color_mode),
                Message::ColorModeSelected,
            ),
            radio(
                "System",
                ColorMode::System,
                Some(self.color_mode),
                Message::ColorModeSelected,
            ),
        ]
        .spacing(12);
        let browser = self.active_browser_settings();
        let theme = self.active_theme_settings();
        let thumbnail_location = browser.thumbnail_location.display().to_string();
        let browser_options = column![
            row![
                column![
                    radio(
                        "List",
                        BrowserLayout::List,
                        Some(browser.layout),
                        Message::BrowserLayoutSelected
                    ),
                    radio(
                        "Tiles",
                        BrowserLayout::Tiles,
                        Some(browser.layout),
                        Message::BrowserLayoutSelected
                    ),
                ]
                .spacing(6)
                .width(Length::Fill),
                self.preference_reset_button(PreferenceOption::Layout),
            ],
            row![
                column![
                    text("Name alignment"),
                    radio(
                        "Left",
                        NameAlignment::Left,
                        Some(browser.name_alignment),
                        Message::NameAlignmentSelected,
                    ),
                    radio(
                        "Center",
                        NameAlignment::Center,
                        Some(browser.name_alignment),
                        Message::NameAlignmentSelected,
                    ),
                    radio(
                        "Right",
                        NameAlignment::Right,
                        Some(browser.name_alignment),
                        Message::NameAlignmentSelected,
                    ),
                ]
                .spacing(6)
                .width(Length::Fill),
                self.preference_reset_button(PreferenceOption::NameAlignment),
            ],
            row![
                text("Item size"),
                slider(20..=64, browser.item_size, Message::BrowserItemSizeChanged)
                    .width(Length::Fill),
                text(format!("{} px", browser.item_size)),
                self.preference_reset_button(PreferenceOption::ItemSize),
            ]
            .spacing(10),
            row![
                toggler(browser.smooth_scrolling)
                    .label("Smooth scrolling")
                    .on_toggle(Message::SmoothScrollingToggled)
                    .width(Length::Fill),
                self.preference_reset_button(PreferenceOption::SmoothScrolling),
            ],
            row![
                text("Scroll step"),
                slider(10..=200, browser.scroll_step, Message::ScrollStepChanged)
                    .width(Length::Fill),
                text(format!("{} px", browser.scroll_step)),
                self.preference_reset_button(PreferenceOption::ScrollStep),
            ]
            .spacing(10),
            row![
                text("Maximum name lines"),
                slider(1..=5, browser.max_name_lines, Message::MaxNameLinesChanged)
                    .width(Length::Fill),
                text(browser.max_name_lines.to_string()),
                self.preference_reset_button(PreferenceOption::MaxNameLines),
            ]
            .spacing(10),
            row![
                toggler(browser.preview_enabled)
                    .label("Show preview pane")
                    .on_toggle(Message::PreviewToggled)
                    .width(Length::Fill),
                self.preference_reset_button(PreferenceOption::Preview),
            ],
            row![
                toggler(browser.single_click_opens_folders)
                    .label("Open folders with one click")
                    .on_toggle(Message::SingleClickFoldersToggled)
                    .width(Length::Fill),
                self.preference_reset_button(PreferenceOption::SingleClickFolders),
            ],
            row![
                pick_list(
                    self.icon_theme_choices(&browser),
                    Some(browser.icon_theme.clone()),
                    Message::IconThemeSelected,
                )
                .placeholder("Icon theme")
                .width(Length::Fill)
                .style(rounded_pick_list_style)
                .menu_style(rounded_pick_list_menu_style),
                self.preference_reset_button(PreferenceOption::IconTheme),
            ],
            row![
                text_input("Thumbnail location", &thumbnail_location,)
                    .on_input(Message::ThumbnailLocationChanged)
                    .width(Length::Fill),
                self.preference_reset_button(PreferenceOption::ThumbnailLocation),
            ],
            row![
                column![
                    pick_list(
                        self.terminal_choices(),
                        Some(self.selected_terminal_choice(&browser)),
                        Message::TerminalChoiceSelected,
                    )
                    .width(Length::Fill)
                    .style(rounded_pick_list_style)
                    .menu_style(rounded_pick_list_menu_style),
                    text_input(
                        "Custom terminal command",
                        (self.selected_terminal_choice(&browser) == CUSTOM_TERMINAL_CHOICE)
                            .then_some(browser.terminal_command.as_str())
                            .unwrap_or_default(),
                    )
                    .on_input(Message::TerminalCommandChanged)
                    .width(Length::Fill),
                ]
                .spacing(8)
                .width(Length::Fill),
                self.preference_reset_button(PreferenceOption::Terminal),
            ],
            button(text("Restart backend")).on_press(Message::RestartBackend),
        ]
        .spacing(10);
        let context_menu_options = |active_items: &[ContextMenuItem],
                                    available_items: &[ContextMenuItem],
                                    is_directory: bool| {
            let mut configured_items = Vec::new();
            for item in active_items {
                if available_items.contains(item) && !configured_items.contains(item) {
                    configured_items.push(*item);
                }
            }
            let configured_item_count = configured_items.len();
            let mut items = configured_items.clone();
            for item in available_items {
                if !items.contains(item) {
                    items.push(*item);
                }
            }
            items
                .into_iter()
                .enumerate()
                .fold(column![].spacing(6), |column, (index, item)| {
                    let enabled = index < configured_item_count;
                    let move_up = enabled
                        .then_some(index > 0)
                        .filter(|can_move| *can_move)
                        .map(|_| Message::MoveContextMenuItem {
                            item,
                            is_directory,
                            move_up: true,
                        });
                    let move_down = enabled
                        .then_some(index + 1 < configured_item_count)
                        .filter(|can_move| *can_move)
                        .map(|_| Message::MoveContextMenuItem {
                            item,
                            is_directory,
                            move_up: false,
                        });
                    column.push(
                        row![
                            checkbox(enabled)
                                .label(item.to_string())
                                .on_toggle(move |enabled| Message::ContextMenuItemToggled {
                                    item,
                                    is_directory,
                                    enabled,
                                })
                                .width(Length::Fill),
                            tooltip(
                                button(icon_text("chevron-up")).on_press_maybe(move_up),
                                text("Move up"),
                                tooltip::Position::Bottom,
                            ),
                            tooltip(
                                button(icon_text("chevron-down")).on_press_maybe(move_down),
                                text("Move down"),
                                tooltip::Position::Bottom,
                            ),
                        ]
                        .spacing(6),
                    )
                })
        };
        let file_context_menu_options = context_menu_options(
            &browser.file_context_menu_items,
            &ContextMenuItem::FILE_OPTIONS,
            false,
        );
        let folder_context_menu_options = context_menu_options(
            &browser.folder_context_menu_items,
            &ContextMenuItem::FOLDER_OPTIONS,
            true,
        );
        let mut configured_quick_toolbar_items = Vec::new();
        for item in &browser.quick_toolbar_items {
            if !configured_quick_toolbar_items.contains(item) {
                configured_quick_toolbar_items.push(*item);
            }
        }
        let configured_quick_toolbar_item_count = configured_quick_toolbar_items.len();
        let mut quick_toolbar_items = configured_quick_toolbar_items.clone();
        for item in QuickToolbarItem::ALL {
            if !quick_toolbar_items.contains(&item) {
                quick_toolbar_items.push(item);
            }
        }
        let quick_toolbar_options = quick_toolbar_items.into_iter().enumerate().fold(
            column![].spacing(6),
            |column, (index, item)| {
                let enabled = index < configured_quick_toolbar_item_count;
                let move_up = enabled
                    .then_some(index > 0)
                    .filter(|can_move| *can_move)
                    .map(|_| Message::MoveQuickToolbarItem(item, true));
                let move_down = enabled
                    .then_some(index + 1 < configured_quick_toolbar_item_count)
                    .filter(|can_move| *can_move)
                    .map(|_| Message::MoveQuickToolbarItem(item, false));
                column.push(
                    row![
                        checkbox(enabled)
                            .label(item.to_string())
                            .on_toggle(move |enabled| {
                                Message::QuickToolbarItemToggled(item, enabled)
                            })
                            .width(Length::Fill),
                        tooltip(
                            button(icon_text("chevron-up")).on_press_maybe(move_up),
                            text("Move up"),
                            tooltip::Position::Bottom,
                        ),
                        tooltip(
                            button(icon_text("chevron-down")).on_press_maybe(move_down),
                            text("Move down"),
                            tooltip::Position::Bottom,
                        ),
                    ]
                    .spacing(6),
                )
            },
        );
        let keyboard_shortcut_options =
            KeyboardShortcutAction::ALL
                .into_iter()
                .fold(column![].spacing(6), |column, action| {
                    let key = browser
                        .keyboard_shortcuts
                        .iter()
                        .find(|shortcut| shortcut.action == action)
                        .map(|shortcut| shortcut.key.as_str())
                        .unwrap_or_default();
                    column.push(
                        row![
                            text(action.to_string()).width(Length::Fill),
                            text_input("Key", key)
                                .on_input(move |key| Message::KeyboardShortcutChanged {
                                    action,
                                    key
                                })
                                .width(Length::Fixed(120.0)),
                        ]
                        .spacing(10),
                    )
                });
        let profiles = self
            .profiles
            .iter()
            .fold(column![].spacing(6), |column, profile| {
                let selected = self.active_profile.as_deref() == Some(profile.path.as_path());
                let label = if selected {
                    format!("{} (active)", profile.name)
                } else {
                    profile.name.clone()
                };
                let profile_button = if profile.read_only {
                    button(
                        row![
                            text(label),
                            tooltip(
                                icon_text("lock").size(16),
                                text("Read-only profile"),
                                tooltip::Position::Right,
                            ),
                        ]
                        .spacing(8),
                    )
                } else {
                    button(row![text(label)].spacing(8))
                }
                .width(Length::Fill)
                .on_press(Message::SelectProfile(profile.path.clone()));
                column.push(profile_button)
            });
        let create_profile = row![
            text_input("New profile name", &self.new_profile_name)
                .on_input(Message::NewProfileNameChanged)
                .on_submit(Message::CreateProfile)
                .width(Length::Fill),
            tooltip(
                button(icon_text("plus")).on_press(Message::CreateProfile),
                text("Create profile"),
                tooltip::Position::Bottom,
            ),
        ]
        .spacing(8);
        let search_paths = self
            .config_store
            .search_paths()
            .iter()
            .fold(column![].spacing(4), |column, path| {
                column.push(text(path.display().to_string()))
            });

        let page = container(
            scrollable(
                column![
                    row![back_button, text("Preferences").size(24)].spacing(12),
                    column![text("Profiles").size(18), profiles, create_profile].spacing(10),
                    column![
                        row![
                            text("Color mode").size(18),
                            button(
                                row![icon_text("rotate-ccw").size(16), text("Reset profile")]
                                    .spacing(6)
                            )
                            .on_press(Message::RequestProfileReset),
                        ]
                        .spacing(8),
                        row![
                            container(options).width(Length::Fill),
                            self.preference_reset_button(PreferenceOption::ColorMode),
                        ],
                        row![
                            container(self.accent_picker_button(false)).width(Length::Fill),
                            self.preference_reset_button(PreferenceOption::LightAccent),
                        ],
                        row![
                            container(self.accent_picker_button(true)).width(Length::Fill),
                            self.preference_reset_button(PreferenceOption::DarkAccent),
                        ],
                        row![
                            text("Background opacity"),
                            slider(
                                0..=100,
                                theme.background_opacity.min(100),
                                Message::BackgroundOpacityChanged,
                            )
                            .width(Length::Fill),
                            text(format!("{}%", theme.background_opacity.min(100))),
                            self.preference_reset_button(PreferenceOption::BackgroundOpacity),
                        ]
                        .spacing(10),
                        row![
                            text("Blur strength"),
                            slider(
                                0..=5,
                                theme.context_menu_blur_strength.min(5),
                                Message::ContextMenuBlurStrengthChanged,
                            )
                            .width(Length::Fill),
                            text(format!("sigma {}", theme.context_menu_blur_strength.min(5))),
                            self.preference_reset_button(PreferenceOption::ContextMenuBlurStrength),
                        ]
                        .spacing(10),
                        row![
                            text("Kernel size"),
                            pick_list(
                                ContextMenuBlurKernelSize::OPTIONS,
                                Some(theme.context_menu_blur_kernel_size),
                                Message::ContextMenuBlurKernelSizeChanged,
                            )
                            .width(Length::Fill)
                            .style(rounded_pick_list_style)
                            .menu_style(rounded_pick_list_menu_style),
                            self.preference_reset_button(
                                PreferenceOption::ContextMenuBlurKernelSize
                            ),
                        ]
                        .spacing(10),
                        row![
                            text("Corner radius"),
                            slider(
                                0..=8,
                                theme.border_radius.min(8),
                                Message::BorderRadiusChanged,
                            )
                            .width(Length::Fill),
                            text(format!("{} px", theme.border_radius.min(8))),
                            self.preference_reset_button(PreferenceOption::BorderRadius),
                        ]
                        .spacing(10),
                    ]
                    .spacing(10),
                    column![text("Browser").size(18), browser_options].spacing(10),
                    column![
                        row![
                            text("Keyboard shortcuts").size(18),
                            self.preference_reset_button(PreferenceOption::KeyboardShortcuts),
                        ]
                        .spacing(8),
                        keyboard_shortcut_options,
                    ]
                    .spacing(10),
                    column![
                        row![
                            text("File context menu").size(18),
                            self.preference_reset_button(PreferenceOption::FileContextMenuItems),
                        ]
                        .spacing(8),
                        file_context_menu_options,
                    ]
                    .spacing(10),
                    column![
                        row![
                            text("Folder context menu").size(18),
                            self.preference_reset_button(PreferenceOption::FolderContextMenuItems),
                        ]
                        .spacing(8),
                        folder_context_menu_options,
                    ]
                    .spacing(10),
                    column![
                        row![
                            text("Quick toolbar").size(18),
                            self.preference_reset_button(PreferenceOption::QuickToolbarItems),
                        ]
                        .spacing(8),
                        quick_toolbar_options,
                    ]
                    .spacing(10),
                    column![text("Configuration search paths").size(18), search_paths].spacing(10),
                ]
                .spacing(28)
                .padding(16)
                .width(Length::Fill),
            )
            .direction(modern_vertical_scrollbar())
            .scroll_step(f32::from(browser.scroll_step))
            .style(modern_scrollable_style),
        )
        .width(Length::Fill)
        .height(Length::Fill);
        let reset_confirmation = self.pending_profile_reset.then(|| {
            let dialog = container(
                column![
                    text("Reset active profile?"),
                    text("This restores the profile from the repository default configuration."),
                    row![
                        button(text("Cancel")).on_press(Message::CancelProfileReset),
                        button(text("Reset profile")).on_press(Message::ConfirmProfileReset),
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
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .on_press(Message::CancelProfileReset),
                container(dialog)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
        });
        let accent_picker = self.accent_picker.map(|picker| {
            let color = hsv_color(picker.hue, picker.saturation, picker.value);
            let hue_color = |hue| hsv_color(hue, picker.saturation, picker.value);
            let hue_gradient = Gradient::Linear(
                Linear::new(std::f32::consts::FRAC_PI_2)
                    .add_stop(0.0, hue_color(0))
                    .add_stop(0.17, hue_color(60))
                    .add_stop(0.33, hue_color(120))
                    .add_stop(0.5, hue_color(180))
                    .add_stop(0.67, hue_color(240))
                    .add_stop(0.83, hue_color(300))
                    .add_stop(1.0, hue_color(360)),
            );
            let hue = stack![
                container(Space::new().width(Length::Fill).height(Length::Fixed(10.0))).style(
                    move |_| { iced::widget::container::Style::default().background(hue_gradient) }
                ),
                slider(0..=360, picker.hue, Message::AccentHueChanged)
                    .width(Length::Fill)
                    .style(|theme, status| {
                        let mut style = iced::widget::slider::default(theme, status);
                        style.rail.backgrounds = (
                            Background::Color(Color::TRANSPARENT),
                            Background::Color(Color::TRANSPARENT),
                        );
                        style
                    }),
            ]
            .width(Length::Fill)
            .height(Length::Fixed(24.0));
            let saturation_gradient = Gradient::Linear(
                Linear::new(std::f32::consts::FRAC_PI_2)
                    .add_stop(0.0, hsv_color(picker.hue, 0, picker.value))
                    .add_stop(1.0, hsv_color(picker.hue, 255, picker.value)),
            );
            let saturation = stack![
                container(Space::new().width(Length::Fill).height(Length::Fixed(10.0))).style(
                    move |_| {
                        iced::widget::container::Style::default().background(saturation_gradient)
                    }
                ),
                slider(0..=255, picker.saturation, Message::AccentSaturationChanged)
                    .width(Length::Fill)
                    .style(|theme, status| {
                        let mut style = iced::widget::slider::default(theme, status);
                        style.rail.backgrounds = (
                            Background::Color(Color::TRANSPARENT),
                            Background::Color(Color::TRANSPARENT),
                        );
                        style
                    }),
            ]
            .width(Length::Fill)
            .height(Length::Fixed(24.0));
            let value_gradient = Gradient::Linear(
                Linear::new(std::f32::consts::FRAC_PI_2)
                    .add_stop(0.0, Color::BLACK)
                    .add_stop(1.0, hsv_color(picker.hue, picker.saturation, 255)),
            );
            let value = stack![
                container(Space::new().width(Length::Fill).height(Length::Fixed(10.0))).style(
                    move |_| {
                        iced::widget::container::Style::default().background(value_gradient)
                    }
                ),
                slider(0..=255, picker.value, Message::AccentValueChanged)
                    .width(Length::Fill)
                    .style(|theme, status| {
                        let mut style = iced::widget::slider::default(theme, status);
                        style.rail.backgrounds = (
                            Background::Color(Color::TRANSPARENT),
                            Background::Color(Color::TRANSPARENT),
                        );
                        style
                    }),
            ]
            .width(Length::Fill)
            .height(Length::Fixed(24.0));
            let dialog = container(
                column![
                    text(if picker.dark {
                        "Dark accent color"
                    } else {
                        "Light accent color"
                    }),
                    container(Space::new().width(Length::Fill).height(Length::Fixed(42.0))).style(
                        move |_| { iced::widget::container::Style::default().background(color) }
                    ),
                    text("Hue"),
                    hue,
                    text("Saturation"),
                    saturation,
                    text("Value"),
                    value,
                    row![
                        button(text("Cancel")).on_press(Message::CancelAccentPicker),
                        button(text("Apply")).on_press(Message::ConfirmAccentPicker),
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
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .on_press(Message::CancelAccentPicker),
                container(dialog)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
        });
        stack![page]
            .push_maybe(reset_confirmation)
            .push_maybe(accent_picker)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
