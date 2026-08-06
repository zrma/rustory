use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::style::Modifier;
use unicode_width::UnicodeWidthStr;

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_REVERSED: &str = "\x1b[7m";

pub(crate) fn buffer_to_plain_text(buffer: &Buffer, line_ending: &str) -> String {
    serialize_buffer(buffer, line_ending, false)
}

pub(crate) fn buffer_to_ansi_text(buffer: &Buffer, line_ending: &str) -> String {
    serialize_buffer(buffer, line_ending, true)
}

fn serialize_buffer(buffer: &Buffer, line_ending: &str, styled: bool) -> String {
    let area = buffer.area;
    let mut output = String::new();
    for y in area.y..area.bottom() {
        let mut line = String::new();
        let mut active_modifiers = Modifier::empty();
        let mut x = area.x;
        while x < area.right() {
            let Some(cell) = buffer.cell((x, y)) else {
                break;
            };
            if matches!(cell.diff_option, CellDiffOption::Skip) {
                x += 1;
                continue;
            }

            let modifiers = cell.modifier & (Modifier::BOLD | Modifier::REVERSED);
            if styled && modifiers != active_modifiers {
                if !active_modifiers.is_empty() {
                    line.push_str(ANSI_RESET);
                }
                if modifiers.contains(Modifier::BOLD) {
                    line.push_str(ANSI_BOLD);
                }
                if modifiers.contains(Modifier::REVERSED) {
                    line.push_str(ANSI_REVERSED);
                }
                active_modifiers = modifiers;
            }

            let symbol = cell.symbol();
            line.push_str(symbol);
            let width = UnicodeWidthStr::width(symbol).max(1);
            x = x.saturating_add(u16::try_from(width).unwrap_or(u16::MAX));
        }
        if styled && !active_modifiers.is_empty() {
            line.push_str(ANSI_RESET);
        }
        if styled {
            output.push_str(&line);
        } else {
            output.push_str(line.trim_end());
        }
        output.push_str(line_ending);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    #[test]
    fn serializes_wide_cells_without_duplicate_padding() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
        buffer.set_string(0, 0, "한글", Style::default());

        assert_eq!(buffer_to_plain_text(&buffer, "\n"), "한글\n");
    }

    #[test]
    fn serializes_supported_inline_styles() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
        buffer.set_string(
            0,
            0,
            "abc",
            Style::default().add_modifier(Modifier::REVERSED),
        );

        assert_eq!(
            buffer_to_ansi_text(&buffer, "\r\n"),
            "\x1b[7mabc\x1b[0m\r\n"
        );
    }
}
