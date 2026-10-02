use super::*;

impl Gui {
    pub(super) fn confirm_picker(&mut self) -> Task<Message> {
        let Some(picker) = self.picker.as_ref() else {
            return Task::none();
        };
        if self
            .save_file_name
            .as_deref()
            .is_some_and(|name| !valid_save_file_name(name))
        {
            self.status = "Enter a simple file name".into();
            return Task::none();
        }
        let mut selected = self
            .selected_entries
            .iter()
            .filter(|path| {
                std::fs::metadata(path)
                    .map(|metadata| (picker.kind == PickerKind::Folder) == metadata.is_dir())
                    .unwrap_or(false)
            })
            .cloned()
            .collect::<Vec<_>>();
        selected.sort();
        if selected.is_empty() {
            if picker.kind == PickerKind::Folder {
                selected.push(self.directory_path.clone());
            } else {
                self.status = "Select a file first".into();
                return Task::none();
            }
        }
        if !picker.multiple {
            selected.truncate(1);
        }
        for path in selected {
            let path = self
                .save_file_name
                .as_deref()
                .map(|name| path.join(name))
                .unwrap_or(path);
            println!("{}", path.display());
        }
        self.close_window()
    }

    pub(super) fn close_window(&self) -> Task<Message> {
        if self.picker.is_some() {
            let _ = self
                .config_store
                .set_last_picker_directory(&self.directory_path);
        }
        iced::exit()
    }

    pub(super) fn execute_browser_command(&mut self, command: BrowserCommand) -> Task<Message> {
        if self.view != View::Browser || self.editing_address {
            return Task::none();
        }
        match command {
            BrowserCommand::CopySelection => {
                let mut entries = self.selected_entries.iter().cloned().collect::<Vec<_>>();
                entries.sort();
                if entries.is_empty() {
                    self.status = "Select files or folders to copy".into();
                } else {
                    self.paste_buffer = Some(PasteBuffer {
                        entries,
                        mode: PasteMode::Copy,
                    });
                    self.context_entry = None;
                    let count = self
                        .paste_buffer
                        .as_ref()
                        .map_or(0, |buffer| buffer.entries.len());
                    self.status = format!("Copied {count} item(s) to the clipboard");
                }
                Task::none()
            }
            BrowserCommand::CutSelection => {
                let mut entries = self.selected_entries.iter().cloned().collect::<Vec<_>>();
                entries.sort();
                if entries.is_empty() {
                    self.status = "Select files or folders to cut".into();
                } else {
                    let count = entries.len();
                    self.paste_buffer = Some(PasteBuffer {
                        entries,
                        mode: PasteMode::Move,
                    });
                    self.context_entry = None;
                    self.status = format!("Cut {count} item(s) to the clipboard");
                }
                Task::none()
            }
            BrowserCommand::CancelCut => {
                if matches!(
                    self.paste_buffer.as_ref().map(|buffer| buffer.mode),
                    Some(PasteMode::Move)
                ) {
                    self.paste_buffer = None;
                    self.status = "Cut cancelled".into();
                }
                Task::none()
            }
            BrowserCommand::RenameSelection => {
                if self.selected_entries.len() != 1 {
                    self.status = "Select exactly one file or folder to rename".into();
                    return Task::none();
                }
                let path = self
                    .selected_entries
                    .iter()
                    .next()
                    .cloned()
                    .unwrap_or_default();
                self.update(Message::RequestRenameEntry(path))
            }
            BrowserCommand::CopyLocation(path) => {
                self.context_entry = None;
                self.status = "Copied location to the clipboard".into();
                iced::clipboard::write(path.to_string_lossy().into_owned())
            }
            BrowserCommand::Paste => {
                let Some(buffer) = self.paste_buffer.clone() else {
                    self.status = "Nothing to paste".into();
                    return Task::none();
                };
                self.context_entry = None;
                self.status = format!("Pasting {} item(s)...", buffer.entries.len());
                match buffer.mode {
                    PasteMode::Copy => Task::perform(
                        copy_entries(buffer.entries, self.directory_path.clone()),
                        Message::FileCopyFinished,
                    ),
                    PasteMode::Move => {
                        let destination = self.directory_path.clone();
                        let sources = buffer
                            .entries
                            .into_iter()
                            .filter(|path| path.parent() != Some(destination.as_path()))
                            .collect::<Vec<_>>();
                        if sources.is_empty() {
                            self.paste_buffer = None;
                            self.status = "Item(s) are already in this folder".into();
                            return Task::none();
                        }
                        let to_delete = sources.clone();
                        Task::perform(copy_entries(sources, destination), move |result| {
                            Message::CutPasteFinished {
                                sources: to_delete.clone(),
                                result,
                            }
                        })
                    }
                    PasteMode::Symlink => Task::perform(
                        create_symlinks(buffer.entries, self.directory_path.clone()),
                        Message::FileCopyFinished,
                    ),
                }
            }
            BrowserCommand::DeleteSelection => {
                let mut paths = self.selected_entries.iter().cloned().collect::<Vec<_>>();
                paths.sort();
                if paths.is_empty() {
                    self.status = "Select files or folders to delete".into();
                } else {
                    self.context_entry = None;
                    self.delete_confirm_selected = false;
                    self.pending_delete = Some(paths);
                }
                Task::none()
            }
            BrowserCommand::AddSymlinkToPasteBuffer(path) => {
                self.paste_buffer = Some(PasteBuffer {
                    entries: vec![path],
                    mode: PasteMode::Symlink,
                });
                self.context_entry = None;
                self.status = "Added symbolic link to the paste buffer".into();
                Task::none()
            }
            BrowserCommand::CreateSymlinksHere(source) => {
                self.context_entry = None;
                self.status = "Creating symbolic link...".into();
                Task::perform(
                    create_symlinks(vec![source], self.directory_path.clone()),
                    Message::FileCopyFinished,
                )
            }
            BrowserCommand::CompressSelection => {
                if self.selected_entries.is_empty() {
                    self.status = "Select files or folders first".into();
                } else {
                    self.pending_compression = true;
                }
                Task::none()
            }
            BrowserCommand::ExtractSelection => {
                let mut paths = self.selected_entries.iter().cloned().collect::<Vec<_>>();
                paths.sort();
                if paths.is_empty() {
                    self.status = "Select files or folders first".into();
                    return Task::none();
                }
                self.status = format!("Extracting {} archive(s)...", paths.len());
                Task::perform(
                    extract_archives(paths, self.directory_path.clone()),
                    |result| Message::ArchiveFinished {
                        action: "Extracted",
                        result,
                    },
                )
            }
        }
    }

    pub(super) fn execute_compression(&mut self) -> Task<Message> {
        let mut paths = self.selected_entries.iter().cloned().collect::<Vec<_>>();
        paths.sort();
        if paths.is_empty() {
            self.status = "Select files or folders first".into();
            return Task::none();
        }
        self.status = format!("Compressing {} item(s)...", paths.len());
        Task::perform(
            compress_entries(
                paths,
                self.directory_path.clone(),
                i32::from(self.compression_level),
                self.compression_type.value().into(),
            ),
            |result| Message::ArchiveFinished {
                action: "Compressed",
                result,
            },
        )
    }
}
