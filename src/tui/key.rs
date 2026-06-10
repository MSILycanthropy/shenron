use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Input parsed out of one SSH data packet.
#[derive(Debug)]
pub(super) enum Input {
    Key(KeyEvent),
    Paste(String),
}

const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

/// Escape sequences longer than this don't exist in the protocols terminput
/// understands (kitty and SGR mouse included); bounds the longest-match scan.
const MAX_SEQUENCE: usize = 32;

/// Parse every key and paste out of a packet. Unknown sequences, mouse
/// reports, and key releases are consumed and dropped, never mangled into
/// phantom keys.
pub(super) fn parse_input(data: &[u8]) -> Vec<Input> {
    let mut inputs = Vec::new();
    let mut rest = data;

    while !rest.is_empty() {
        let (input, consumed) = if rest[0] == 0x1b {
            parse_escape(rest)
        } else {
            parse_char(rest)
        };

        if let Some(input) = input {
            inputs.push(input);
        }

        rest = &rest[consumed.max(1)..];
    }

    inputs
}

/// terminput's parser reports no consumed-byte count and tolerates trailing
/// garbage, so sequence boundaries come from shortest-match: grow the prefix
/// until the first complete parse. `Ok(None)` means "incomplete, keep
/// growing"; `Err` means the sequence can never parse, so skip it wholesale.
/// The lone ESC is excluded from the scan — it would preempt every sequence.
fn parse_escape(data: &[u8]) -> (Option<Input>, usize) {
    if data.starts_with(PASTE_START) {
        return parse_paste(data);
    }

    if data.len() == 1 {
        return (Some(esc_key()), 1);
    }

    let limit = data.len().min(MAX_SEQUENCE);

    for end in 2..=limit {
        match terminput::Event::parse_from(&data[..end]) {
            Ok(Some(event)) => return (convert(event), end),
            Ok(None) => {}
            Err(_) => break,
        }
    }

    // Nothing terminput recognizes. Skip a CSI sequence in one piece rather
    // than emit a phantom Esc followed by its payload as keys; anything else
    // is a real Esc keypress followed by ordinary bytes.
    if data[1] == b'[' {
        (None, skip_csi(data))
    } else {
        (Some(esc_key()), 1)
    }
}

const fn esc_key() -> Input {
    Input::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
}

/// Bracketed paste gets a fast path: longest-match over a multi-kilobyte
/// paste would be quadratic, and the end marker tells us the span directly.
fn parse_paste(data: &[u8]) -> (Option<Input>, usize) {
    let Some(end) = data
        .windows(PASTE_END.len())
        .position(|window| window == PASTE_END)
    else {
        // Paste split across packets; drop it rather than mangle it.
        return (None, data.len());
    };

    let total = end + PASTE_END.len();

    match terminput::Event::parse_from(&data[..total]) {
        Ok(Some(event)) => (convert(event), total),
        _ => (None, total),
    }
}

/// Plain bytes parse one UTF-8 code point at a time — handing terminput more
/// would succeed but silently ignore everything after the first character.
fn parse_char(data: &[u8]) -> (Option<Input>, usize) {
    let width = match data[0] {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => return (None, 1),
    };

    if data.len() < width {
        // Truncated code point at the end of the packet.
        return (None, data.len());
    }

    match terminput::Event::parse_from(&data[..width]) {
        Ok(Some(event)) => (convert(event), width),
        _ => (None, width),
    }
}

/// Consume `ESC [ <params> <intermediates> <final>`; final bytes are
/// `0x40..=0x7E` per ECMA-48.
fn skip_csi(data: &[u8]) -> usize {
    data.iter()
        .skip(2)
        .position(|b| (0x40..=0x7E).contains(b))
        .map_or(data.len(), |i| i + 3)
}

fn convert(event: terminput::Event) -> Option<Input> {
    match event {
        terminput::Event::Key(key) if key.kind != terminput::KeyEventKind::Release => {
            convert_key(&key).map(Input::Key)
        }
        terminput::Event::Paste(text) => Some(Input::Paste(text)),
        // Mouse, focus, resize-via-CSI, and key releases are not part of the
        // Tui event surface.
        _ => None,
    }
}

fn convert_key(key: &terminput::KeyEvent) -> Option<KeyEvent> {
    use terminput::KeyCode as T;

    let shift = key.modifiers.contains(terminput::KeyModifiers::SHIFT);

    let code = match key.code {
        T::Tab if shift => KeyCode::BackTab,
        T::Tab => KeyCode::Tab,
        T::Backspace => KeyCode::Backspace,
        T::Enter => KeyCode::Enter,
        T::Left => KeyCode::Left,
        T::Right => KeyCode::Right,
        T::Up => KeyCode::Up,
        T::Down => KeyCode::Down,
        T::Home => KeyCode::Home,
        T::End => KeyCode::End,
        T::PageUp => KeyCode::PageUp,
        T::PageDown => KeyCode::PageDown,
        T::Delete => KeyCode::Delete,
        T::Insert => KeyCode::Insert,
        T::F(n) => KeyCode::F(n),
        T::Char(c) => KeyCode::Char(c),
        T::Esc => KeyCode::Esc,
        _ => return None,
    };

    Some(KeyEvent::new(code, convert_modifiers(key.modifiers)))
}

fn convert_modifiers(modifiers: terminput::KeyModifiers) -> KeyModifiers {
    use terminput::KeyModifiers as T;

    [
        (T::SHIFT, KeyModifiers::SHIFT),
        (T::CTRL, KeyModifiers::CONTROL),
        (T::ALT, KeyModifiers::ALT),
        (T::SUPER, KeyModifiers::SUPER),
        (T::HYPER, KeyModifiers::HYPER),
        (T::META, KeyModifiers::META),
    ]
    .into_iter()
    .filter(|(from, _)| modifiers.contains(*from))
    .fold(KeyModifiers::NONE, |acc, (_, to)| acc | to)
}

#[cfg(test)]
mod tests {
    use super::{Input, parse_input};
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    fn keys(data: &[u8]) -> Vec<(KeyCode, KeyModifiers)> {
        parse_input(data)
            .into_iter()
            .filter_map(|input| match input {
                Input::Key(key) => Some((key.code, key.modifiers)),
                Input::Paste(_) => None,
            })
            .collect()
    }

    #[test]
    fn empty_input_is_nothing() {
        assert!(parse_input(&[]).is_empty());
    }

    #[test]
    fn pasted_text_yields_every_key() {
        let codes: Vec<_> = keys(b"hello").into_iter().map(|(c, _)| c).collect();

        let expected: Vec<_> = "hello".chars().map(KeyCode::Char).collect();
        assert_eq!(codes, expected);
    }

    #[test]
    fn multibyte_utf8_chars_parse_individually() {
        let codes: Vec<_> = keys("héllo".as_bytes()).into_iter().map(|(c, _)| c).collect();

        let expected: Vec<_> = "héllo".chars().map(KeyCode::Char).collect();
        assert_eq!(codes, expected);
    }

    #[test]
    fn keys_mixed_with_sequences_all_arrive() {
        let parsed = keys(b"a\x1b[Ab");

        assert_eq!(
            parsed,
            vec![
                (KeyCode::Char('a'), KeyModifiers::NONE),
                (KeyCode::Up, KeyModifiers::NONE),
                (KeyCode::Char('b'), KeyModifiers::NONE),
            ]
        );
    }

    #[test]
    fn arrow_up_escape_sequence() {
        assert_eq!(keys(b"\x1b[A"), vec![(KeyCode::Up, KeyModifiers::NONE)]);
    }

    #[test]
    fn modified_arrow_carries_modifier() {
        // ESC [ 1 ; 3 A = Alt+Up — previously a phantom Char(ESC).
        assert_eq!(keys(b"\x1b[1;3A"), vec![(KeyCode::Up, KeyModifiers::ALT)]);
    }

    #[test]
    fn bare_esc_is_esc() {
        assert_eq!(keys(b"\x1b"), vec![(KeyCode::Esc, KeyModifiers::NONE)]);
    }

    #[test]
    fn mouse_reports_are_dropped() {
        // SGR mouse press: parsed by terminput as a mouse event, not a key.
        assert!(parse_input(b"\x1b[<0;1;1M").is_empty());
    }

    #[test]
    fn unknown_csi_is_skipped_not_mangled() {
        // Private-mode set: not an input event in any protocol.
        assert!(parse_input(b"\x1b[?2004h").is_empty());
    }

    #[test]
    fn bracketed_paste_is_one_event() {
        let inputs = parse_input(b"\x1b[200~hi there\x1b[201~");

        assert!(
            matches!(&inputs[..], [Input::Paste(text)] if text == "hi there")
        );
    }

    #[test]
    fn keys_after_a_paste_still_arrive() {
        let inputs = parse_input(b"\x1b[200~hi\x1b[201~x");

        assert_eq!(inputs.len(), 2);
        assert!(matches!(&inputs[0], Input::Paste(text) if text == "hi"));
        assert!(
            matches!(&inputs[1], Input::Key(key) if key.code == KeyCode::Char('x'))
        );
    }

    #[test]
    fn ctrl_c_is_control_char() {
        assert_eq!(
            keys(&[3]),
            vec![(KeyCode::Char('c'), KeyModifiers::CONTROL)]
        );
    }

    #[test]
    fn uppercase_letter_carries_shift() {
        // crossterm convention: the char stays uppercase, SHIFT is set.
        assert_eq!(
            keys(b"A"),
            vec![(KeyCode::Char('A'), KeyModifiers::SHIFT)]
        );
    }

    #[test]
    fn shift_tab_is_backtab() {
        assert_eq!(
            keys(b"\x1b[Z"),
            vec![(KeyCode::BackTab, KeyModifiers::SHIFT)]
        );
    }
}
