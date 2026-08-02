use super::*;

pub(super) fn icon_text<'a>(name: &str) -> iced::widget::Text<'a> {
    let icon = try_icon(Pack::Lucide, name, Style::Regular, Size::Regular)
        .expect("missing bundled Lucide icon");
    let glyph = char::from_u32(icon.codepoint).unwrap_or('?');

    text(glyph.to_string())
        .size(18)
        .font(Font::with_name(icon.family))
}

pub(super) fn truncate_label(value: &str, max_characters: usize) -> String {
    let character_count = value.chars().count();
    if character_count <= max_characters {
        return value.into();
    }

    let visible_characters = max_characters.saturating_sub(3);
    format!(
        "{}...",
        value.chars().take(visible_characters).collect::<String>()
    )
}

pub(super) fn sort_entries(entries: &mut [proto::FileEntry], order: EntrySortOrder) {
    entries.sort_by(|left, right| {
        let group = (!left.is_directory, left.name.starts_with('.'))
            .cmp(&(!right.is_directory, right.name.starts_with('.')));
        if group.is_ne() {
            return group;
        }
        let names = left.name.to_lowercase().cmp(&right.name.to_lowercase());
        match order {
            EntrySortOrder::NameAscending => names,
            EntrySortOrder::NameDescending => names.reverse(),
            EntrySortOrder::ModifiedNewest => right.modified_at.cmp(&left.modified_at).then(names),
            EntrySortOrder::ModifiedOldest => left.modified_at.cmp(&right.modified_at).then(names),
            EntrySortOrder::CreatedNewest => right.created_at.cmp(&left.created_at).then(names),
            EntrySortOrder::CreatedOldest => left.created_at.cmp(&right.created_at).then(names),
        }
    });
}

pub(super) fn available_icon_themes() -> Vec<String> {
    let mut themes = vec!["bundled".into()];
    #[cfg(target_os = "linux")]
    {
        let theme = Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "icon-theme"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .trim_matches('\'')
                    .to_owned()
            })
            .filter(|theme| !theme.is_empty());
        if let Some(theme) = theme
            && !themes.contains(&theme)
        {
            themes.push(theme);
        }
    }
    themes
}

pub(super) fn themed_entry_icon_path(theme: &str, entry: &proto::FileEntry) -> Option<PathBuf> {
    if theme == "bundled" {
        return None;
    }
    let icons = gio_icon_names(Path::new(&entry.path));
    for root in icon_theme_directories(theme) {
        for size in ["128x128", "96x96", "64x64", "48x48", "scalable"] {
            for icon in &icons {
                for category in ["mimetypes", "places", "actions", "status", "apps"] {
                    let path = root.join(size).join(category).join(format!("{icon}.svg"));
                    if path.is_file() {
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}

pub(super) fn gio_icon_names(path: &Path) -> Vec<String> {
    let Ok(output) = Command::new("gio")
        .args(["info", "-a", "standard::icon"])
        .arg(path)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.trim().strip_prefix("standard::icon:"))
        .map(|icons| {
            icons
                .split(',')
                .map(str::trim)
                .filter(|icon| !icon.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn icon_theme_directories(theme: &str) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        directories.push(home.join(".nix-profile/share/icons").join(theme));
        directories.push(home.join(".local/share/icons").join(theme));
    }
    directories.push(PathBuf::from("/run/current-system/sw/share/icons").join(theme));
    directories
}

pub(super) fn file_item_button_style(
    theme: &Theme,
    status: button_style::Status,
    selected: bool,
) -> button_style::Style {
    let base = button_style::text(theme, status);
    let style = button_style::Style {
        border: Border {
            radius: border_radius().into(),
            ..base.border
        },
        ..base
    };
    if selected {
        button_style::Style {
            text_color: contrasting_text_color(theme.palette().primary),
            ..style.with_background(theme.palette().primary)
        }
    } else if matches!(status, button_style::Status::Hovered) {
        style.with_background(Color::from_rgba8(128, 128, 128, 0.18))
    } else {
        style
    }
}

pub(super) fn contrasting_text_color(background: Color) -> Color {
    let linear_channel = |channel: f32| {
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * linear_channel(background.r)
        + 0.7152 * linear_channel(background.g)
        + 0.0722 * linear_channel(background.b);

    if luminance > 0.179 {
        Color::BLACK
    } else {
        Color::WHITE
    }
}

pub(super) fn context_menu_button_style(
    theme: &Theme,
    status: button_style::Status,
) -> button_style::Style {
    let style = button_style::Style {
        border: Border {
            radius: border_radius().into(),
            ..button_style::text(theme, status).border
        },
        ..button_style::text(theme, status)
    };
    if matches!(status, button_style::Status::Hovered) {
        style.with_background(Color::from_rgba8(128, 128, 128, 0.18))
    } else {
        style
    }
}

pub(super) fn modern_scrollable_style(
    theme: &Theme,
    status: iced::widget::scrollable::Status,
) -> iced::widget::scrollable::Style {
    let palette = theme.extended_palette();
    let thumb_color = match status {
        iced::widget::scrollable::Status::Active => {
            palette.background.strong.color.scale_alpha(0.45)
        }
        iced::widget::scrollable::Status::Hovered { .. } => {
            theme.palette().primary.scale_alpha(0.7)
        }
        iced::widget::scrollable::Status::Dragged { .. } => theme.palette().primary,
    };
    let rail = iced::widget::scrollable::Rail {
        background: None,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 4.0.into(),
        },
        scroller: iced::widget::scrollable::Scroller {
            color: thumb_color,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 4.0.into(),
            },
        },
    };
    iced::widget::scrollable::Style {
        container: iced::widget::container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
    }
}

pub(super) fn modern_vertical_scrollbar() -> iced::widget::scrollable::Direction {
    iced::widget::scrollable::Direction::Vertical(
        iced::widget::scrollable::Scrollbar::new()
            .width(7.0)
            .scroller_width(5.0)
            .margin(2.0),
    )
}

pub(super) fn rounded_pick_list_style(
    theme: &Theme,
    status: iced::widget::pick_list::Status,
) -> iced::widget::pick_list::Style {
    let palette = theme.extended_palette();
    iced::widget::pick_list::Style {
        text_color: palette.background.weak.text,
        placeholder_color: palette.background.strong.color,
        handle_color: palette.background.weak.text,
        background: Color::from_rgba8(128, 128, 128, 0.12).into(),
        border: Border {
            color: if matches!(
                status,
                iced::widget::pick_list::Status::Hovered | iced::widget::pick_list::Status::Opened
            ) {
                theme.palette().primary
            } else {
                palette.background.strong.color.scale_alpha(0.6)
            },
            width: 1.0,
            radius: border_radius().into(),
        },
    }
}

pub(super) fn rounded_pick_list_menu_style(theme: &Theme) -> iced::widget::overlay::menu::Style {
    let palette = theme.extended_palette();
    iced::widget::overlay::menu::Style {
        background: palette.background.base.color.into(),
        border: Border {
            color: palette.background.strong.color.scale_alpha(0.7),
            width: 1.0,
            radius: border_radius().into(),
        },
        text_color: palette.background.base.text,
        selected_text_color: contrasting_text_color(theme.palette().primary),
        selected_background: theme.palette().primary.into(),
    }
}

pub(super) fn sidebar_icon(location: &SidebarLocation) -> &'static str {
    match location.label.as_str() {
        "Home" => "house",
        "Downloads" => "download",
        "Pictures" => "image",
        _ => "folder",
    }
}

pub(super) fn parse_color(value: &str) -> Option<Color> {
    let value = value.strip_prefix('#')?;
    if value.len() != 6 {
        return None;
    }
    let red = u8::from_str_radix(&value[0..2], 16).ok()?;
    let green = u8::from_str_radix(&value[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&value[4..6], 16).ok()?;
    Some(Color::from_rgb8(red, green, blue))
}

pub(super) fn hsv_color(hue: u16, saturation: u8, value: u8) -> Color {
    let hue = (hue % 360) as f32 / 60.0;
    let value = f32::from(value) / 255.0;
    let chroma = value * f32::from(saturation) / 255.0;
    let secondary = chroma * (1.0 - ((hue % 2.0) - 1.0).abs());
    let (red, green, blue) = match hue as u8 {
        0 => (chroma, secondary, 0.0),
        1 => (secondary, chroma, 0.0),
        2 => (0.0, chroma, secondary),
        3 => (0.0, secondary, chroma),
        4 => (secondary, 0.0, chroma),
        _ => (chroma, 0.0, secondary),
    };
    let offset = value - chroma;
    Color::from_rgb(red + offset, green + offset, blue + offset)
}

pub(super) fn rgb_to_hsv(color: Color) -> (u16, u8, u8) {
    let maximum = color.r.max(color.g).max(color.b);
    let minimum = color.r.min(color.g).min(color.b);
    let delta = maximum - minimum;
    let hue = if delta == 0.0 {
        0.0
    } else if maximum == color.r {
        60.0 * ((color.g - color.b) / delta).rem_euclid(6.0)
    } else if maximum == color.g {
        60.0 * ((color.b - color.r) / delta + 2.0)
    } else {
        60.0 * ((color.r - color.g) / delta + 4.0)
    };
    let saturation = if maximum == 0.0 { 0.0 } else { delta / maximum };
    (
        hue.round() as u16,
        (saturation * 255.0).round() as u8,
        (maximum * 255.0).round() as u8,
    )
}

#[derive(Debug, Deserialize)]
pub(super) struct LsblkOutput {
    #[serde(default)]
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Debug, Deserialize)]
pub(super) struct LsblkDevice {
    name: String,
    path: Option<PathBuf>,
    label: Option<String>,
    #[serde(default)]
    mountpoints: Vec<Option<PathBuf>>,
    #[serde(default)]
    children: Vec<LsblkDevice>,
    #[serde(rename = "type")]
    device_type: String,
}

pub(super) async fn load_mounts() -> Result<MountState, String> {
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("lsblk")
            .args(["--json", "--output", "NAME,PATH,LABEL,MOUNTPOINTS,TYPE"])
            .output()
            .map_err(|error| format!("Could not list drives: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        let output: LsblkOutput = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("Could not read drive list: {error}"))?;
        let mut drives = Vec::new();
        for device in output.blockdevices {
            collect_drives(device, &mut drives);
        }
        let mut mounts = drives
            .iter()
            .flat_map(|drive| drive.mount_points.iter().cloned())
            .map(|path| SystemMount {
                path,
                filesystem: "block".into(),
            })
            .collect::<Vec<_>>();
        mounts.extend(read_remote_mounts()?);
        mounts.sort_by(|left, right| left.path.cmp(&right.path));
        mounts.dedup_by(|left, right| left.path == right.path);
        Ok(MountState { drives, mounts })
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(MountState {
            drives: Vec::new(),
            mounts: Vec::new(),
        })
    }
}

#[cfg(target_os = "linux")]
pub(super) fn collect_drives(device: LsblkDevice, drives: &mut Vec<Drive>) {
    let is_volume = matches!(device.device_type.as_str(), "disk" | "part")
        && (device.device_type == "part" || device.children.is_empty());
    if is_volume {
        if let Some(path) = device.path {
            let mount_points = device.mountpoints.into_iter().flatten().collect();
            drives.push(Drive {
                path,
                name: device.label.unwrap_or(device.name),
                mount_points,
            });
        }
    }
    for child in device.children {
        collect_drives(child, drives);
    }
}

pub(super) async fn mount_drive(path: PathBuf) -> Result<MountState, String> {
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("udisksctl")
            .args(["mount", "--block"])
            .arg(&path)
            .output()
            .map_err(|error| format!("Could not mount {}: {error}", path.display()))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        load_mounts().await
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err("Mounting drives is not supported on this platform".into())
    }
}

pub(super) async fn open_file(path: PathBuf) -> Result<(), String> {
    let status = Command::new("xdg-open")
        .arg(&path)
        .status()
        .map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("xdg-open could not open {}", path.display()))
    }
}

pub(super) async fn open_terminal(path: PathBuf, configured_command: String) -> Result<(), String> {
    let command = configured_command.trim();
    if !command.is_empty() && command != "default" {
        return spawn_terminal_command(command, &path);
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(command) = gnome_default_terminal_command()
            .filter(|command| terminal_command_is_available(command))
        {
            return spawn_terminal_command(&command, &path);
        }
        for command in ["xdg-terminal-exec", "x-terminal-emulator"]
            .into_iter()
            .chain(RECOMMENDED_TERMINALS.iter().copied())
        {
            if terminal_command_is_available(command) {
                return spawn_terminal_command(command, &path);
            }
        }
        Err(format!(
            "No supported terminal command is available for {}",
            path.display()
        ))
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .args(["-a", "Terminal"])
            .arg(&path)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Could not open Terminal in {}: {error}", path.display()))
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", "cmd", "/K"])
            .current_dir(&path)
            .spawn()
            .map(|_| ())
            .map_err(|error| {
                format!(
                    "Could not open Command Prompt in {}: {error}",
                    path.display()
                )
            })
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = path;
        Err("Opening a terminal is not supported on this platform".into())
    }
}

#[cfg(target_os = "linux")]
pub(super) fn recommended_terminal_commands() -> Vec<String> {
    let script = format!(
        "for app in {}; do command -v \"$app\" 2>/dev/null; done",
        RECOMMENDED_TERMINALS.join(" ")
    );
    let Ok(output) = Command::new("sh").args(["-c", &script]).output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

#[cfg(not(target_os = "linux"))]
pub(super) fn recommended_terminal_commands() -> Vec<String> {
    Vec::new()
}

pub(super) fn spawn_terminal_command(command: &str, path: &Path) -> Result<(), String> {
    Command::new("sh")
        .args(["-c", command])
        .current_dir(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open {command} in {}: {error}", path.display()))
}

#[cfg(target_os = "linux")]
pub(super) fn gnome_default_terminal_command() -> Option<String> {
    let output = Command::new("gsettings")
        .args([
            "get",
            "org.gnome.desktop.default-applications.terminal",
            "exec",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let command = String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_matches('\'')
        .to_owned();
    (!command.is_empty()).then_some(command)
}

#[cfg(target_os = "linux")]
pub(super) fn terminal_command_is_available(command: &str) -> bool {
    let Some(executable) = command.split_whitespace().next() else {
        return false;
    };
    let executable_path = Path::new(executable);
    if executable_path.components().count() > 1 {
        return executable_path.is_file();
    }
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| directory.join(executable).is_file())
    })
}

pub(super) async fn default_file_opener(path: PathBuf) -> Result<String, String> {
    let mime_output = Command::new("file")
        .args(["--mime-type", "-b"])
        .arg(&path)
        .output()
        .map_err(|error| {
            format!(
                "Could not determine the type of {}: {error}",
                path.display()
            )
        })?;
    if !mime_output.status.success() {
        return Err(format!(
            "Could not determine the type of {}",
            path.display()
        ));
    }
    let mime = String::from_utf8_lossy(&mime_output.stdout)
        .trim()
        .to_owned();
    if mime.is_empty() {
        return Err(format!("No MIME type was returned for {}", path.display()));
    }

    let application_output = Command::new("xdg-mime")
        .args(["query", "default", &mime])
        .output()
        .map_err(|error| format!("Could not find an application for {mime}: {error}"))?;
    if !application_output.status.success() {
        return Err(format!("Could not find an application for {mime}"));
    }
    let application = String::from_utf8_lossy(&application_output.stdout)
        .trim()
        .to_owned();
    if application.is_empty() {
        Err(format!("No default application is configured for {mime}"))
    } else {
        Ok(desktop_entry_name(&application).unwrap_or(application))
    }
}

pub(super) fn desktop_entry_name(application: &str) -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let user = std::env::var_os("USER")?;
    let output = Command::new("find")
        .arg(PathBuf::from(&home).join(".local/share/applications"))
        .arg(PathBuf::from(&home).join(".nix-profile/share/applications"))
        .arg(
            PathBuf::from("/etc/profiles/per-user")
                .join(user)
                .join("share/applications"),
        )
        .arg("/run/current-system/sw/share/applications")
        .args([
            "-name",
            application,
            "-exec",
            "awk",
            "-F=",
            "/^Name=/{print substr($0,6); exit}",
            "{}",
            ";",
            "-quit",
        ])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!name.is_empty()).then_some(name)
}

#[cfg(target_os = "linux")]
pub(super) fn read_remote_mounts() -> Result<Vec<SystemMount>, String> {
    let mountinfo = fs::read_to_string("/proc/self/mountinfo")
        .map_err(|error| format!("Could not read mounted filesystems: {error}"))?;
    Ok(mountinfo
        .lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let mount_path = left.split_whitespace().nth(4)?;
            let mut filesystem_fields = right.split_whitespace();
            let filesystem = filesystem_fields.next()?;
            let _source = filesystem_fields.next()?;
            let mount = SystemMount {
                path: PathBuf::from(unescape_mount_path(mount_path)),
                filesystem: filesystem.into(),
            };
            (mount.filesystem == "fuse.rclone").then_some(mount)
        })
        .collect())
}

#[cfg(target_os = "linux")]
pub(super) fn unescape_mount_path(path: &str) -> String {
    path.replace(r"\040", " ")
        .replace(r"\011", "\t")
        .replace(r"\012", "\n")
        .replace(r"\134", r"\")
}
