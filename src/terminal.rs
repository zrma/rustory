pub(crate) fn contains_terminal_control(value: &str) -> bool {
    value.chars().any(is_terminal_control)
}

pub(crate) fn sanitize_one_line(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\n' | '\r' | '\t' => out.push(' '),
            '\u{1b}' => out.push_str("\\x1b"),
            '\u{00}'..='\u{1f}' | '\u{7f}' => {
                out.push_str(&format!("\\x{:02x}", ch as u32));
            }
            '\u{80}'..='\u{9f}' => {
                out.push_str(&format!("\\u{{{:02x}}}", ch as u32));
            }
            _ => out.push(ch),
        }
    }
    out
}

fn is_terminal_control(ch: char) -> bool {
    matches!(ch, '\u{00}'..='\u{1f}' | '\u{7f}'..='\u{9f}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_c0_c1_and_del_controls() {
        for value in ["a\0b", "a\x1bb", "a\u{7f}b", "a\u{85}b"] {
            assert!(contains_terminal_control(value), "{value:?}");
        }
        assert!(!contains_terminal_control("한글 peer 1.2.3"));
    }

    #[test]
    fn sanitizes_terminal_controls_as_one_line_text() {
        assert_eq!(
            sanitize_one_line("a\nb\rc\td\x1b]52;c;AAAA\x07\u{85}"),
            "a b c d\\x1b]52;c;AAAA\\x07\\u{85}"
        );
    }
}
