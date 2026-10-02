use super::*;

pub(super) fn border_radius() -> f32 {
    f32::from(BORDER_RADIUS.load(Ordering::Relaxed))
}

pub(super) fn set_border_radius(radius: u8) {
    BORDER_RADIUS.store(radius.min(8), Ordering::Relaxed);
}

pub(super) fn button<'a, Message>(
    content: impl Into<Element<'a, Message>>,
) -> iced::widget::Button<'a, Message> {
    iced::widget::button(content).style(rounded_button_style)
}

pub(super) fn rounded_button_style(
    theme: &Theme,
    status: button_style::Status,
) -> button_style::Style {
    let base = button_style::primary(theme, status);
    button_style::Style {
        border: Border {
            radius: border_radius().into(),
            ..base.border
        },
        ..base
    }
}

pub(super) fn rounded_text_button_style(
    theme: &Theme,
    status: button_style::Status,
) -> button_style::Style {
    let base = button_style::text(theme, status);
    button_style::Style {
        border: Border {
            radius: border_radius().into(),
            ..base.border
        },
        ..base
    }
}

pub(super) fn rounded_text_input_style(
    theme: &Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    let base = iced::widget::text_input::default(theme, status);
    iced::widget::text_input::Style {
        border: Border {
            radius: border_radius().into(),
            ..base.border
        },
        ..base
    }
}
