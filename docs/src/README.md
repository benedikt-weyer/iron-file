# Iron File

Iron File is a graphical file browser with an Iced frontend. It can also run as
a file or folder picker for automation and as an XDG Desktop Portal FileChooser
backend.

Normal GUI launches detach from the terminal. Use `-f` or `--follow` to keep
the process attached and stream backend logs. See [Command Line Picker](picker.md)
for picker mode and [XDG Desktop Portal](xdg-desktop-portal.md) for NixOS setup.

## Theme settings

Profile theme settings are stored in the `[theme]` section. `background_opacity`
controls the application background opacity. `context_menu_blur_strength`
is the Gaussian sigma for file and folder context menus, from `0` to `5`, while
`context_menu_blur_kernel_size` selects a fixed `3x3` through `31x31` Gaussian
kernel or the dynamic string `"6sigma+1"`. Dynamic mode computes a window of
`6 * sigma + 1`, capped at `31x31`. They apply only when `background_opacity`
is below `100`. The defaults are sigma `2` and `"6sigma+1"`.
The `patches/iced-wgpu-native-backdrop-blur.patch` renderer patch exposes the
native frame snapshot required for the context-menu backdrop shader. Iron File
vendors and enables that patch through its Cargo workspace override.

## Context menu

The `[browser]` `file_context_menu_items` and `folder_context_menu_items`
arrays independently control visible actions and their order. Omit an item to
hide it. Folder defaults put `"create-folder"` and `"create-file"` first.
Available values are `"create-folder"`, `"create-file"`, `"rename"`, `"open"`,
`"copy-location"`, `"copy-selection"`, `"delete-selection"`, `"paste"`,
`"toggle-sidebar-location"`, `"create-symlink"`,
`"add-symlink-to-paste-buffer"`, and `"open-terminal"`. Actions that do not
apply to the selected file or folder are hidden automatically. The legacy
`context_menu_items` setting is read as the initial list for both menus.

The `[browser]` `quick_toolbar_items` array controls the visible quick-toolbar
actions and their order. Available values are `"toggle-hidden-files"`,
`"sort"`, `"compress-selection"`, and `"extract-selection"`. The
`sort_order` setting selects `"name-ascending"` or `"name-descending"`.

The `[browser]` `keyboard_shortcuts` array maps actions to keys. Its default
binding is `{ action = "rename-selection", key = "F2" }`. Set `key` to an
empty string to disable an action.
