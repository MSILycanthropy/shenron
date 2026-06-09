use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[must_use]
pub fn parse_key_event(data: &[u8]) -> Option<KeyEvent> {
    if data.is_empty() {
        return None;
    }

    let key = match data {
        // Arrow keys
        [27, 91, 65] => KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
        [27, 91, 66] => KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        [27, 91, 67] => KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        [27, 91, 68] => KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),

        // Arrow keys with modifiers (some terminals)
        [27, 91, 49, 59, 50, 65] => KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT),
        [27, 91, 49, 59, 50, 66] => KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT),
        [27, 91, 49, 59, 50, 67] => KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT),
        [27, 91, 49, 59, 50, 68] => KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT),
        [27, 91, 49, 59, 53, 65] => KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL),
        [27, 91, 49, 59, 53, 66] => KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL),
        [27, 91, 49, 59, 53, 67] => KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL),
        [27, 91, 49, 59, 53, 68] => KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL),

        // Home/End
        [27, 91 | 79, 72] => KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
        [27, 91 | 79, 70] => KeyEvent::new(KeyCode::End, KeyModifiers::NONE),

        // Insert/Delete/PageUp/PageDown
        [27, 91, 50, 126] => KeyEvent::new(KeyCode::Insert, KeyModifiers::NONE),
        [27, 91, 51, 126] => KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE),
        [27, 91, 53, 126] => KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE),
        [27, 91, 54, 126] => KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),

        // Function keys
        [27, 79, 80] => KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE),
        [27, 79, 81] => KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE),
        [27, 79, 82] => KeyEvent::new(KeyCode::F(3), KeyModifiers::NONE),
        [27, 79, 83] => KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE),
        [27, 91, 49, 53, 126] => KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE),
        [27, 91, 49, 55, 126] => KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE),
        [27, 91, 49, 56, 126] => KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE),
        [27, 91, 49, 57, 126] => KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE),
        [27, 91, 50, 48, 126] => KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE),
        [27, 91, 50, 49, 126] => KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE),
        [27, 91, 50, 51, 126] => KeyEvent::new(KeyCode::F(11), KeyModifiers::NONE),
        [27, 91, 50, 52, 126] => KeyEvent::new(KeyCode::F(12), KeyModifiers::NONE),

        // Escape
        [27] => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),

        // Enter
        [13] => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),

        // Tab / Shift+Tab
        [9] => KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        [27, 91, 90] => KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),

        // Backspace
        [127 | 8] => KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),

        // Space
        [32] => KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),

        // Ctrl+letter (Ctrl+A = 1, Ctrl+B = 2, ..., Ctrl+Z = 26)
        [b @ 1..=26] => {
            let c = (b'a' + b - 1) as char;
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
        }

        // Regular ASCII character
        [c] if c.is_ascii_graphic() || *c == b' ' => {
            let c = *c as char;
            if c.is_ascii_uppercase() {
                KeyEvent::new(KeyCode::Char(c.to_ascii_lowercase()), KeyModifiers::SHIFT)
            } else {
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
            }
        }

        // Alt+letter (ESC followed by letter)
        [27, c] if c.is_ascii_alphabetic() => {
            let c = (*c as char).to_ascii_lowercase();
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
        }

        _ => return parse_utf8_char(data),
    };

    Some(key)
}

fn parse_utf8_char(data: &[u8]) -> Option<KeyEvent> {
    let s = std::str::from_utf8(data).ok()?;

    let c = s.chars().next()?;

    let modifiers = if c.is_ascii_uppercase() {
        KeyModifiers::SHIFT
    } else {
        KeyModifiers::NONE
    };

    let c = if c.is_ascii_uppercase() {
        c.to_ascii_lowercase()
    } else {
        c
    };

    Some(KeyEvent::new(KeyCode::Char(c), modifiers))
}

#[cfg(test)]
mod tests {
    use super::parse_key_event;
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    #[test]
    fn empty_input_is_none() {
        assert!(parse_key_event(&[]).is_none());
    }

    #[test]
    fn arrow_up_escape_sequence() {
        let key = parse_key_event(&[27, 91, 65]).expect("arrow up");
        assert_eq!(key.code, KeyCode::Up);
        assert_eq!(key.modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn ctrl_c_is_control_char() {
        let key = parse_key_event(&[3]).expect("ctrl-c");
        assert_eq!(key.code, KeyCode::Char('c'));
        assert_eq!(key.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn uppercase_letter_carries_shift() {
        let key = parse_key_event(b"A").expect("uppercase A");
        assert_eq!(key.code, KeyCode::Char('a'));
        assert_eq!(key.modifiers, KeyModifiers::SHIFT);
    }
}
