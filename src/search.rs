use crate::core::Entry;
use crate::terminal::sanitize_one_line;
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::time::Duration;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const TABLE_MAX_ROWS: usize = 20;
const TABLE_SCROLL_STEP: usize = 10;
const MOUSE_SCROLL_STEP: usize = 3;
const MIN_FRAME_WIDTH: usize = 40;
const FALLBACK_TERM_WIDTH: usize = 100;
const FALLBACK_TERM_HEIGHT: usize = 30;
const CWD_COLUMN: usize = 1;
const COMMAND_COLUMN: usize = 5;
const FOOTER: &str = "rustory: Search your shell history  • ctrl+h help";
const TTY_LINE_ENDING: &str = "\r\n";
const ENABLE_MOUSE_REPORTING: &[u8] = b"\x1b[?1000h\x1b[?1006h";
const DISABLE_MOUSE_REPORTING: &[u8] = b"\x1b[?1000l\x1b[?1006l";
const COMMAND_FIELD_WEIGHT: usize = 6_000;
const COMPACT_CWD_FIELD_WEIGHT: usize = 4_000;
const CWD_FIELD_WEIGHT: usize = 3_900;
const HOSTNAME_FIELD_WEIGHT: usize = 3_000;
const DEVICE_FIELD_WEIGHT: usize = 2_600;
const USER_FIELD_WEIGHT: usize = 1_800;
const EXPLICIT_FIELD_WEIGHT: usize = 6_000;
const SAME_CWD_BOOST: usize = 80;
const SAME_HOSTNAME_BOOST: usize = 20;
const MAX_TEXT_MATCH_QUALITY: usize = 2_100;

const COLUMNS: [ColumnSpec; 6] = [
    ColumnSpec {
        title: "Hostname",
        min_width: 8,
        preferred_width: 14,
    },
    ColumnSpec {
        title: "CWD",
        min_width: 6,
        preferred_width: 22,
    },
    ColumnSpec {
        title: "Timestamp",
        min_width: 19,
        preferred_width: 23,
    },
    ColumnSpec {
        title: "Runtime",
        min_width: 7,
        preferred_width: 8,
    },
    ColumnSpec {
        title: "Exit Code",
        min_width: 4,
        preferred_width: 9,
    },
    ColumnSpec {
        title: "Command",
        min_width: 10,
        preferred_width: 28,
    },
];

#[derive(Debug, Clone, Copy)]
struct ColumnSpec {
    title: &'static str,
    min_width: usize,
    preferred_width: usize,
}

#[derive(Debug, Clone)]
struct SearchRow {
    entry_index: usize,
    search_fields: SearchFields,
    cells: [String; 6],
}

#[derive(Debug, Clone)]
struct SearchFields {
    command: String,
    command_ascii_mask: u128,
    cwd: String,
    cwd_ascii_mask: u128,
    compact_cwd: String,
    compact_cwd_ascii_mask: u128,
    hostname: String,
    hostname_ascii_mask: u128,
    device_id: String,
    device_ascii_mask: u128,
    user_id: String,
    user_ascii_mask: u128,
    metadata_ascii_mask: u128,
    exit_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchTerm {
    negate: bool,
    matcher: SearchMatcher,
}

#[derive(Debug)]
struct PreparedSearchTerm<'a> {
    negate: bool,
    field: Option<SearchField>,
    token: PreparedToken<'a>,
}

#[derive(Debug)]
struct PreparedToken<'a> {
    value: &'a str,
    chars: Vec<char>,
    all_alphanumeric: bool,
    ascii_mask: Option<u128>,
}

impl<'a> PreparedToken<'a> {
    fn new(value: &'a str) -> Self {
        let chars = value.chars().collect::<Vec<_>>();
        let all_alphanumeric = chars.iter().all(|ch| ch.is_alphanumeric());
        let ascii_mask = value.is_ascii().then(|| ascii_mask(value));
        Self {
            value,
            chars,
            all_alphanumeric,
            ascii_mask,
        }
    }

    fn len(&self) -> usize {
        self.chars.len()
    }

    fn may_match_ascii_mask(&self, haystack_mask: u128) -> bool {
        let Some(token_mask) = self.ascii_mask else {
            return true;
        };
        let missing = (token_mask & !haystack_mask).count_ones();
        // 한 글자 오타 후보는 query에만 있는 문자 하나를 허용해야 false negative가 없다.
        let allowed_missing = usize::from(self.len() >= 4 && self.all_alphanumeric);
        missing as usize <= allowed_missing
    }
}

fn ascii_mask(value: &str) -> u128 {
    value
        .bytes()
        .filter(|byte| byte.is_ascii())
        .fold(0u128, |mask, byte| mask | (1u128 << byte))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SearchMatcher {
    Any(String),
    Field { field: SearchField, value: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchField {
    Command,
    Cwd,
    Hostname,
    User,
    Device,
    ExitCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Char(char),
    Backspace,
    Delete,
    DeleteSelected,
    Enter,
    Quit,
    Help,
    Up,
    Down,
    Left,
    Right,
    CtrlLeft,
    CtrlRight,
    ShiftLeft,
    ShiftRight,
    PageUp,
    PageDown,
    MouseWheelUp,
    MouseWheelDown,
    Home,
    End,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchAction {
    Select(String),
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SearchContext {
    cwd: Option<String>,
    hostname: Option<String>,
}

impl SearchContext {
    pub(crate) fn new(cwd: Option<String>, hostname: Option<String>) -> Self {
        Self {
            cwd: normalize_context_value(cwd),
            hostname: normalize_context_value(hostname),
        }
    }
}

pub fn select_action(
    entries: &[Entry],
    context: SearchContext,
    mut delete_entry: impl FnMut(&str) -> Result<()>,
) -> Result<Option<SearchAction>> {
    if entries.is_empty() {
        return Ok(None);
    }

    let rows = build_search_rows(entries);
    let mut tui = SearchTui::new(entries, rows, context)?;
    tui.run(&mut delete_entry)
}

fn build_search_rows(entries: &[Entry]) -> Vec<SearchRow> {
    entries
        .iter()
        .enumerate()
        .map(|(entry_index, entry)| {
            let compact = compact_cwd(&entry.cwd);
            let command = sanitize_one_line(&entry.cmd).to_lowercase();
            let cwd = sanitize_one_line(&entry.cwd).to_lowercase();
            let compact_cwd = compact.to_lowercase();
            let hostname = sanitize_one_line(&entry.hostname).to_lowercase();
            let device_id = sanitize_one_line(&entry.device_id).to_lowercase();
            let user_id = sanitize_one_line(&entry.user_id).to_lowercase();
            SearchRow {
                entry_index,
                search_fields: SearchFields {
                    command_ascii_mask: ascii_mask(&command),
                    cwd_ascii_mask: ascii_mask(&cwd),
                    compact_cwd_ascii_mask: ascii_mask(&compact_cwd),
                    hostname_ascii_mask: ascii_mask(&hostname),
                    device_ascii_mask: ascii_mask(&device_id),
                    user_ascii_mask: ascii_mask(&user_id),
                    metadata_ascii_mask: ascii_mask(&cwd)
                        | ascii_mask(&compact_cwd)
                        | ascii_mask(&hostname)
                        | ascii_mask(&device_id)
                        | ascii_mask(&user_id),
                    command,
                    cwd,
                    compact_cwd,
                    hostname,
                    device_id,
                    user_id,
                    exit_code: entry.exit_code.to_string(),
                },
                cells: [
                    sanitize_one_line(&entry.hostname),
                    compact,
                    format_timestamp(entry.ts),
                    format_duration(entry.duration_ms),
                    entry.exit_code.to_string(),
                    sanitize_one_line(&entry.cmd),
                ],
            }
        })
        .collect()
}

struct SearchTui<'a> {
    entries: &'a [Entry],
    rows: Vec<SearchRow>,
    filtered: Vec<usize>,
    query: Vec<char>,
    query_cursor: usize,
    cursor: usize,
    scroll_top: usize,
    hscroll: usize,
    visible_rows: usize,
    show_help: bool,
    tty: Tty,
    last_rendered_lines: usize,
    deleted_entry_ids: HashSet<String>,
    context: SearchContext,
}

impl<'a> SearchTui<'a> {
    fn new(entries: &'a [Entry], rows: Vec<SearchRow>, context: SearchContext) -> Result<Self> {
        let tty = Tty::open().context("open controlling terminal for search TUI")?;
        let mut tui = Self {
            entries,
            rows,
            filtered: Vec::new(),
            query: Vec::new(),
            query_cursor: 0,
            cursor: 0,
            scroll_top: 0,
            hscroll: 0,
            visible_rows: TABLE_MAX_ROWS,
            show_help: false,
            tty,
            last_rendered_lines: 0,
            deleted_entry_ids: HashSet::new(),
            context,
        };
        tui.refresh_matches(true);
        Ok(tui)
    }

    fn run(
        &mut self,
        delete_entry: &mut impl FnMut(&str) -> Result<()>,
    ) -> Result<Option<SearchAction>> {
        self.render()?;
        loop {
            match self.tty.read_key().context("read search TUI key")? {
                Key::Enter => {
                    let action = self.selected_command().map(SearchAction::Select);
                    self.finish()?;
                    return Ok(action);
                }
                Key::DeleteSelected => {
                    if let Some(entry_id) = self.selected_delete_entry_id() {
                        delete_entry(&entry_id)?;
                        self.deleted_entry_ids.insert(entry_id);
                        self.refresh_matches(false);
                    }
                }
                Key::Quit => {
                    self.finish()?;
                    return Ok(None);
                }
                key => self.handle_key(key),
            }
            self.render()?;
        }
    }

    fn finish(&mut self) -> Result<()> {
        self.tty
            .write_all(clear_rendered_frame(self.last_rendered_lines).as_bytes())?;
        self.tty.write_all(DISABLE_MOUSE_REPORTING)?;
        self.tty.write_all(b"\x1b[?25h\x1b[0m")?;
        self.tty.flush()?;
        self.last_rendered_lines = 0;
        Ok(())
    }

    fn selected_command(&self) -> Option<String> {
        Some(self.selected_entry()?.cmd.clone())
    }

    fn selected_delete_entry_id(&self) -> Option<String> {
        Some(self.selected_entry()?.entry_id.clone())
    }

    fn selected_entry(&self) -> Option<&Entry> {
        selected_entry(self.entries, &self.rows, &self.filtered, self.cursor)
    }

    fn handle_key(&mut self, key: Key) {
        match key {
            Key::Char(ch) => {
                self.query.insert(self.query_cursor, ch);
                self.query_cursor += 1;
                self.refresh_matches(true);
            }
            Key::Backspace => {
                if self.query_cursor > 0 {
                    self.query_cursor -= 1;
                    self.query.remove(self.query_cursor);
                    self.refresh_matches(true);
                }
            }
            Key::Delete => {
                if self.query_cursor < self.query.len() {
                    self.query.remove(self.query_cursor);
                    self.refresh_matches(true);
                }
            }
            Key::Help => {
                self.show_help = !self.show_help;
            }
            Key::Up => self.move_cursor_up(1),
            Key::Down => self.move_cursor_down(1),
            Key::PageUp => self.move_cursor_up(self.visible_rows.max(1)),
            Key::PageDown => self.move_cursor_down(self.visible_rows.max(1)),
            Key::MouseWheelUp => self.move_cursor_up(MOUSE_SCROLL_STEP),
            Key::MouseWheelDown => self.move_cursor_down(MOUSE_SCROLL_STEP),
            Key::Home => {
                self.cursor = 0;
                self.ensure_cursor_visible();
            }
            Key::End => {
                self.cursor = self.filtered.len().saturating_sub(1);
                self.ensure_cursor_visible();
            }
            Key::Left => {
                self.query_cursor = self.query_cursor.saturating_sub(1);
            }
            Key::Right => {
                self.query_cursor = (self.query_cursor + 1).min(self.query.len());
            }
            Key::CtrlLeft => {
                self.query_cursor = previous_word_boundary(&self.query, self.query_cursor);
            }
            Key::CtrlRight => {
                self.query_cursor = next_word_boundary(&self.query, self.query_cursor);
            }
            Key::ShiftLeft => {
                self.hscroll = self.hscroll.saturating_sub(TABLE_SCROLL_STEP);
            }
            Key::ShiftRight => {
                self.hscroll = self.hscroll.saturating_add(TABLE_SCROLL_STEP);
            }
            Key::Enter | Key::DeleteSelected | Key::Quit | Key::Unknown => {}
        }
    }

    fn move_cursor_up(&mut self, n: usize) {
        self.cursor = self.cursor.saturating_sub(n);
        self.ensure_cursor_visible();
    }

    fn move_cursor_down(&mut self, n: usize) {
        if self.filtered.is_empty() {
            self.cursor = 0;
        } else {
            self.cursor = (self.cursor + n).min(self.filtered.len() - 1);
        }
        self.ensure_cursor_visible();
    }

    fn refresh_matches(&mut self, reset_cursor: bool) {
        let query = self.query_string();
        self.filtered = filter_rows_excluding_deleted_with_context(
            self.entries,
            &self.rows,
            &query,
            &self.deleted_entry_ids,
            &self.context,
        );
        if reset_cursor {
            self.cursor = 0;
            self.scroll_top = 0;
        } else {
            self.cursor = self.cursor.min(self.filtered.len().saturating_sub(1));
        }
        self.ensure_cursor_visible();
    }

    fn query_string(&self) -> String {
        self.query.iter().collect()
    }

    fn ensure_cursor_visible(&mut self) {
        if self.filtered.is_empty() {
            self.cursor = 0;
            self.scroll_top = 0;
            return;
        }
        self.cursor = self.cursor.min(self.filtered.len() - 1);
        if self.cursor < self.scroll_top {
            self.scroll_top = self.cursor;
        }
        if self.cursor >= self.scroll_top.saturating_add(self.visible_rows) {
            self.scroll_top = self
                .cursor
                .saturating_sub(self.visible_rows.saturating_sub(1));
        }
    }

    fn render(&mut self) -> Result<()> {
        let (term_width, term_height) =
            terminal_size(self.tty.fd()).unwrap_or((FALLBACK_TERM_WIDTH, FALLBACK_TERM_HEIGHT));
        let frame = self.render_frame(term_width, term_height);

        self.tty
            .write_all(redraw_prefix(self.last_rendered_lines).as_bytes())?;
        self.tty.write_all(b"\x1b[?25l")?;
        self.tty.write_all(frame.as_bytes())?;
        self.tty.flush()?;
        self.last_rendered_lines = frame.bytes().filter(|b| *b == b'\n').count();
        Ok(())
    }

    fn render_frame(&mut self, term_width: usize, term_height: usize) -> String {
        let frame_width = term_width.saturating_sub(1).max(MIN_FRAME_WIDTH);
        let inside_width = frame_width.saturating_sub(2).max(1);
        self.visible_rows = table_visible_rows(term_height, self.show_help);
        self.ensure_cursor_visible();

        let column_widths = compute_column_widths(&self.rows, &self.filtered, inside_width);
        self.hscroll = self.hscroll.min(max_hscroll(
            &self.rows,
            &self.filtered,
            &column_widths,
            self.scroll_top,
            self.visible_rows,
        ));

        let mut lines = vec![
            format!(
                "Search Query: {}",
                render_query(&self.query, self.query_cursor)
            ),
            String::new(),
            top_border(frame_width),
            table_line(
                &render_cells(
                    COLUMNS.map(|col| col.title.to_string()).as_ref(),
                    &column_widths,
                    inside_width,
                    0,
                ),
                false,
            ),
            separator_border(frame_width),
        ];

        for display_row in 0..self.visible_rows {
            let filtered_index = self.scroll_top + display_row;
            if let Some(row_index) = self.filtered.get(filtered_index).copied() {
                let selected = filtered_index == self.cursor;
                let content = render_cells(
                    &self.rows[row_index].cells,
                    &column_widths,
                    inside_width,
                    self.hscroll,
                );
                lines.push(table_line(&content, selected));
            } else {
                lines.push(table_line(&" ".repeat(inside_width), false));
            }
        }

        lines.push(bottom_border(frame_width));
        lines.push(FOOTER.to_string());
        if self.show_help {
            lines.push(
                "↑/↓ or mouse wheel scroll  pgup/pgdn page  enter select  ctrl+k delete  esc exit"
                    .to_string(),
            );
            lines.push("←/→ edit query  ctrl+←/→ jump word  shift+←/→ scroll table".to_string());
        }

        render_frame_lines(&lines)
    }
}

fn redraw_prefix(last_rendered_lines: usize) -> String {
    if last_rendered_lines > 0 {
        format!("\r\x1b[{last_rendered_lines}F\x1b[J")
    } else {
        // Shell widgets invoke `rr search` while the prompt line is still active.
        // Start the inline TUI on a fresh line, then redraw in-place from there.
        TTY_LINE_ENDING.to_string()
    }
}

fn render_frame_lines(lines: &[String]) -> String {
    let mut frame = lines.join(TTY_LINE_ENDING);
    frame.push_str(TTY_LINE_ENDING);
    frame
}

fn clear_rendered_frame(last_rendered_lines: usize) -> String {
    if last_rendered_lines == 0 {
        String::new()
    } else {
        let lines_to_clear = last_rendered_lines + 1;
        format!("\r\x1b[{lines_to_clear}F\x1b[J")
    }
}

struct Tty {
    file: File,
    original: libc::termios,
}

impl Tty {
    fn open() -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("open /dev/tty")?;
        let fd = file.as_raw_fd();

        let mut original = unsafe { std::mem::zeroed::<libc::termios>() };
        if unsafe { libc::tcgetattr(fd, &mut original) } != 0 {
            return Err(std::io::Error::last_os_error()).context("tcgetattr /dev/tty");
        }

        let mut raw = original;
        raw.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        raw.c_oflag &= !libc::OPOST;
        raw.c_cflag |= libc::CS8;
        raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;

        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
            return Err(std::io::Error::last_os_error()).context("tcsetattr raw mode");
        }
        if let Err(err) = file
            .write_all(ENABLE_MOUSE_REPORTING)
            .and_then(|_| file.flush())
        {
            unsafe {
                libc::tcsetattr(fd, libc::TCSANOW, &original);
            }
            return Err(err).context("enable mouse reporting");
        }

        Ok(Self { file, original })
    }

    fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    fn read_key(&mut self) -> std::io::Result<Key> {
        let byte = self.read_byte_blocking()?;
        if byte == 0x1b {
            self.read_escape_sequence()
        } else if byte.is_ascii() {
            Ok(parse_plain_key(byte))
        } else {
            self.read_utf8_key(byte)
        }
    }

    fn read_utf8_key(&mut self, first: u8) -> std::io::Result<Key> {
        let Some(expected_len) = utf8_scalar_len(first) else {
            return Ok(Key::Unknown);
        };
        let mut bytes = Vec::with_capacity(expected_len);
        bytes.push(first);
        while bytes.len() < expected_len {
            let Some(byte) = self.read_byte_with_timeout(Duration::from_millis(30))? else {
                return Ok(Key::Unknown);
            };
            bytes.push(byte);
        }
        Ok(parse_utf8_key(&bytes))
    }

    fn read_escape_sequence(&mut self) -> std::io::Result<Key> {
        let Some(first) = self.read_byte_with_timeout(Duration::from_millis(30))? else {
            return Ok(Key::Quit);
        };
        let mut bytes = vec![first];
        while bytes.len() < 32 {
            let Some(byte) = self.read_byte_with_timeout(Duration::from_millis(2))? else {
                break;
            };
            bytes.push(byte);
            if byte.is_ascii_alphabetic() || byte == b'~' {
                break;
            }
        }
        Ok(parse_escape_sequence(&bytes))
    }

    fn read_byte_blocking(&mut self) -> std::io::Result<u8> {
        let mut buf = [0_u8; 1];
        self.file.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    fn read_byte_with_timeout(&mut self, timeout: Duration) -> std::io::Result<Option<u8>> {
        let fd = self.fd();
        let mut readfds = unsafe { std::mem::zeroed::<libc::fd_set>() };
        unsafe {
            libc::FD_ZERO(&mut readfds);
            libc::FD_SET(fd, &mut readfds);
        }
        let mut tv = libc::timeval {
            tv_sec: timeout.as_secs() as libc::time_t,
            tv_usec: timeout.subsec_micros() as libc::suseconds_t,
        };
        let ready = unsafe {
            libc::select(
                fd + 1,
                &mut readfds,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut tv,
            )
        };
        if ready < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if ready == 0 {
            return Ok(None);
        }
        self.read_byte_blocking().map(Some)
    }
}

impl Write for Tty {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

impl Drop for Tty {
    fn drop(&mut self) {
        let _ = self.file.write_all(DISABLE_MOUSE_REPORTING);
        let _ = self.file.write_all(b"\x1b[?25h\x1b[0m");
        let _ = self.file.flush();
        unsafe {
            libc::tcsetattr(self.fd(), libc::TCSANOW, &self.original);
        }
    }
}

fn parse_escape_sequence(bytes: &[u8]) -> Key {
    if let Some(key) = parse_sgr_mouse_sequence(bytes) {
        return key;
    }

    match bytes {
        b"[A" | b"OA" => Key::Up,
        b"[B" | b"OB" => Key::Down,
        b"[C" | b"OC" => Key::Right,
        b"[D" | b"OD" => Key::Left,
        b"[H" | b"OH" | b"[1~" => Key::Home,
        b"[F" | b"OF" | b"[4~" => Key::End,
        b"[3~" => Key::Delete,
        b"[5~" => Key::PageUp,
        b"[6~" => Key::PageDown,
        b"[1;2D" | b"[2D" => Key::ShiftLeft,
        b"[1;2C" | b"[2C" => Key::ShiftRight,
        b"[1;5D" | b"[5D" => Key::CtrlLeft,
        b"[1;5C" | b"[5C" => Key::CtrlRight,
        _ => Key::Unknown,
    }
}

fn parse_plain_key(byte: u8) -> Key {
    match byte {
        b'\r' | b'\n' => Key::Enter,
        0x01 => Key::Home,
        0x03 | 0x04 => Key::Quit,
        0x05 => Key::End,
        0x08 => Key::Help,
        0x0b => Key::DeleteSelected,
        0x0e => Key::Down,
        0x10 => Key::Up,
        0x7f => Key::Backspace,
        byte if byte.is_ascii_graphic() || byte == b' ' => Key::Char(byte as char),
        _ => Key::Unknown,
    }
}

fn utf8_scalar_len(first: u8) -> Option<usize> {
    match first {
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}

fn parse_utf8_key(bytes: &[u8]) -> Key {
    let Ok(value) = std::str::from_utf8(bytes) else {
        return Key::Unknown;
    };
    let mut chars = value.chars();
    let Some(ch) = chars.next() else {
        return Key::Unknown;
    };
    if chars.next().is_some() || ch.is_control() {
        Key::Unknown
    } else {
        Key::Char(ch)
    }
}

fn parse_sgr_mouse_sequence(bytes: &[u8]) -> Option<Key> {
    let body = bytes.strip_prefix(b"[<")?;
    let (final_byte, payload) = body.split_last()?;
    if *final_byte != b'M' {
        return None;
    }

    let mut parts = payload.split(|byte| *byte == b';');
    let button = parse_ascii_usize(parts.next()?)?;
    parts.next()?;
    parts.next()?;
    if parts.next().is_some() {
        return None;
    }

    if button & 64 == 0 {
        return None;
    }
    match button & 0b11 {
        0 => Some(Key::MouseWheelUp),
        1 => Some(Key::MouseWheelDown),
        _ => None,
    }
}

fn parse_ascii_usize(bytes: &[u8]) -> Option<usize> {
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

fn terminal_size(fd: RawFd) -> Option<(usize, usize)> {
    let mut size = unsafe { std::mem::zeroed::<libc::winsize>() };
    if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut size) } != 0 {
        return None;
    }
    let width = usize::from(size.ws_col);
    let height = usize::from(size.ws_row);
    if width == 0 || height == 0 {
        None
    } else {
        Some((width, height))
    }
}

#[cfg(test)]
fn filter_rows(rows: &[SearchRow], query: &str) -> Vec<usize> {
    filter_rows_with_context(rows, query, &SearchContext::default())
}

fn filter_rows_with_context(
    rows: &[SearchRow],
    query: &str,
    context: &SearchContext,
) -> Vec<usize> {
    let terms = parse_search_terms(query);
    if terms.is_empty() {
        return (0..rows.len()).collect();
    }
    let prepared_terms = prepare_search_terms(&terms);
    let plain_phrase = plain_query_phrase(&terms);
    let prepared_phrase = plain_phrase.as_deref().map(PreparedToken::new);

    let mut scored = rows
        .iter()
        .enumerate()
        .filter_map(|(idx, row)| {
            match_row_score(row, &prepared_terms, prepared_phrase.as_ref(), context)
                .map(|score| (idx, score))
        })
        .collect::<Vec<_>>();

    scored.sort_unstable_by(|(left_idx, left_score), (right_idx, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| left_idx.cmp(right_idx))
    });
    scored.into_iter().map(|(idx, _)| idx).collect()
}

#[cfg(test)]
fn filter_rows_excluding_deleted(
    entries: &[Entry],
    rows: &[SearchRow],
    query: &str,
    deleted_entry_ids: &HashSet<String>,
) -> Vec<usize> {
    filter_rows_excluding_deleted_with_context(
        entries,
        rows,
        query,
        deleted_entry_ids,
        &SearchContext::default(),
    )
}

fn filter_rows_excluding_deleted_with_context(
    entries: &[Entry],
    rows: &[SearchRow],
    query: &str,
    deleted_entry_ids: &HashSet<String>,
    context: &SearchContext,
) -> Vec<usize> {
    let mut filtered = filter_rows_with_context(rows, query, context);
    if deleted_entry_ids.is_empty() {
        return filtered;
    }

    filtered.retain(|row_index| {
        rows.get(*row_index)
            .and_then(|row| entries.get(row.entry_index))
            .is_some_and(|entry| !deleted_entry_ids.contains(&entry.entry_id))
    });
    filtered
}

fn parse_search_terms(query: &str) -> Vec<SearchTerm> {
    tokenize_query(query)
        .into_iter()
        .filter_map(|token| parse_search_term(&token))
        .collect()
}

fn parse_search_term(token: &str) -> Option<SearchTerm> {
    let mut token = token.trim();
    if token.is_empty() {
        return None;
    }

    let negate = token.starts_with('-') && token != "-";
    if negate {
        token = &token[1..];
    }

    let atom_parts = split_unescaped(token, ':', Some(2));
    let matcher = if atom_parts.len() == 2 {
        parse_search_field(&unescape_query(&atom_parts[0]))
            .map(|field| SearchMatcher::Field {
                field,
                value: normalize_query_token(&atom_parts[1]),
            })
            .unwrap_or_else(|| SearchMatcher::Any(normalize_query_token(token)))
    } else {
        SearchMatcher::Any(normalize_query_token(token))
    };

    match &matcher {
        SearchMatcher::Any(value) if value.is_empty() => None,
        SearchMatcher::Field { value, .. } if value.is_empty() => None,
        _ => Some(SearchTerm { negate, matcher }),
    }
}

fn parse_search_field(field: &str) -> Option<SearchField> {
    match field.to_lowercase().as_str() {
        "command" | "cmd" => Some(SearchField::Command),
        "cwd" | "path" => Some(SearchField::Cwd),
        "host" | "hostname" => Some(SearchField::Hostname),
        "user" | "user_id" => Some(SearchField::User),
        "device" | "device_id" => Some(SearchField::Device),
        "exit_code" | "code" => Some(SearchField::ExitCode),
        _ => None,
    }
}

fn normalize_query_token(token: &str) -> String {
    unescape_query(token).to_lowercase()
}

fn prepare_search_terms(terms: &[SearchTerm]) -> Vec<PreparedSearchTerm<'_>> {
    terms
        .iter()
        .map(|term| match &term.matcher {
            SearchMatcher::Any(value) => PreparedSearchTerm {
                negate: term.negate,
                field: None,
                token: PreparedToken::new(value),
            },
            SearchMatcher::Field { field, value } => PreparedSearchTerm {
                negate: term.negate,
                field: Some(*field),
                token: PreparedToken::new(value),
            },
        })
        .collect()
}

fn plain_query_phrase(terms: &[SearchTerm]) -> Option<String> {
    let mut values = Vec::with_capacity(terms.len());
    for term in terms {
        if term.negate {
            return None;
        }
        let SearchMatcher::Any(value) = &term.matcher else {
            return None;
        };
        values.push(value.as_str());
    }
    (values.len() > 1).then(|| values.join(" "))
}

fn match_row_score(
    row: &SearchRow,
    terms: &[PreparedSearchTerm<'_>],
    plain_phrase: Option<&PreparedToken<'_>>,
    context: &SearchContext,
) -> Option<usize> {
    let mut total = 0usize;
    for term in terms {
        let score = match term.field {
            Some(field) => field_score(&row.search_fields, field, &term.token),
            None => any_field_score(&row.search_fields, &term.token),
        };
        if term.negate {
            if score > 0 {
                return None;
            }
        } else if score == 0 {
            return None;
        } else {
            total = total.saturating_add(score);
        }
    }
    let phrase_bonus = plain_phrase
        .and_then(|phrase| contiguous_match_score(&row.search_fields.command, phrase))
        .unwrap_or(0);
    Some(
        total
            .saturating_add(phrase_bonus)
            .saturating_add(context_score(&row.search_fields, context)),
    )
}

fn any_field_score(fields: &SearchFields, token: &PreparedToken<'_>) -> usize {
    let command_score = if token.may_match_ascii_mask(fields.command_ascii_mask) {
        text_match_score(&fields.command, token, COMMAND_FIELD_WEIGHT)
    } else {
        0
    };
    if command_score > 0 {
        return command_score;
    }
    if !token.may_match_ascii_mask(fields.metadata_ascii_mask) {
        return 0;
    }

    let compact_cwd_score = if token.may_match_ascii_mask(fields.compact_cwd_ascii_mask) {
        text_match_score(&fields.compact_cwd, token, COMPACT_CWD_FIELD_WEIGHT)
    } else {
        0
    };
    let full_cwd_score = if token.may_match_ascii_mask(fields.cwd_ascii_mask) {
        text_match_score(&fields.cwd, token, CWD_FIELD_WEIGHT)
    } else {
        0
    };
    let cwd_score = compact_cwd_score.max(full_cwd_score);
    if cwd_score > HOSTNAME_FIELD_WEIGHT + MAX_TEXT_MATCH_QUALITY {
        return cwd_score;
    }

    [
        cwd_score,
        if token.may_match_ascii_mask(fields.hostname_ascii_mask) {
            text_match_score(&fields.hostname, token, HOSTNAME_FIELD_WEIGHT)
        } else {
            0
        },
        if token.may_match_ascii_mask(fields.device_ascii_mask) {
            text_match_score(&fields.device_id, token, DEVICE_FIELD_WEIGHT)
        } else {
            0
        },
        if token.may_match_ascii_mask(fields.user_ascii_mask) {
            text_match_score(&fields.user_id, token, USER_FIELD_WEIGHT)
        } else {
            0
        },
    ]
    .into_iter()
    .max()
    .unwrap_or(0)
}

fn field_score(fields: &SearchFields, field: SearchField, token: &PreparedToken<'_>) -> usize {
    match field {
        SearchField::Command => text_match_score(&fields.command, token, EXPLICIT_FIELD_WEIGHT),
        SearchField::Cwd => {
            let trimmed = PreparedToken::new(token.value.trim_end_matches('/'));
            text_match_score(&fields.cwd, &trimmed, EXPLICIT_FIELD_WEIGHT).max(text_match_score(
                &fields.compact_cwd,
                &trimmed,
                EXPLICIT_FIELD_WEIGHT,
            ))
        }
        SearchField::Hostname => text_match_score(&fields.hostname, token, EXPLICIT_FIELD_WEIGHT),
        SearchField::User => text_match_score(&fields.user_id, token, EXPLICIT_FIELD_WEIGHT),
        SearchField::Device => text_match_score(&fields.device_id, token, EXPLICIT_FIELD_WEIGHT),
        SearchField::ExitCode => {
            if fields.exit_code == token.value.trim() {
                EXPLICIT_FIELD_WEIGHT
            } else {
                0
            }
        }
    }
}

fn text_match_score(haystack: &str, token: &PreparedToken<'_>, base: usize) -> usize {
    if token.value.is_empty() {
        return 0;
    }

    if let Some(score) = contiguous_match_score(haystack, token) {
        return base.saturating_add(score);
    }

    if let Some(score) = typo_match_score(haystack, token) {
        return base.saturating_add(score);
    }

    if let Some(score) = fuzzy_token_score(haystack, token) {
        return base.saturating_add(score);
    }
    0
}

fn contiguous_match_score(haystack: &str, token: &PreparedToken<'_>) -> Option<usize> {
    let token_len = token.len();
    let length_bonus = token_len.min(32) * 4;
    let mut best = None;

    for (byte_pos, _) in haystack.match_indices(token.value) {
        let byte_end = byte_pos + token.value.len();
        let starts_at_boundary = is_text_boundary_before(haystack, byte_pos);
        let ends_at_boundary = is_text_boundary_after(haystack, byte_end);
        let quality = if byte_pos == 0 && byte_end == haystack.len() {
            1_800
        } else if starts_at_boundary && ends_at_boundary {
            1_500
        } else if starts_at_boundary {
            1_300
        } else if ends_at_boundary {
            1_050
        } else {
            900
        };
        let char_pos = haystack[..byte_pos].chars().count();
        let score = quality + length_bonus + 80usize.saturating_sub(char_pos.min(80));
        best = Some(best.map_or(score, |current: usize| current.max(score)));
    }

    best
}

fn is_text_boundary_before(haystack: &str, byte_pos: usize) -> bool {
    byte_pos == 0
        || haystack[..byte_pos]
            .chars()
            .next_back()
            .is_none_or(|ch| !ch.is_alphanumeric())
}

fn is_text_boundary_after(haystack: &str, byte_end: usize) -> bool {
    byte_end == haystack.len()
        || haystack[byte_end..]
            .chars()
            .next()
            .is_none_or(|ch| !ch.is_alphanumeric())
}

fn typo_match_score(haystack: &str, token: &PreparedToken<'_>) -> Option<usize> {
    let token_len = token.len();
    if token_len < 4 || !token.all_alphanumeric {
        return None;
    }

    haystack
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .any(|word| edit_distance_at_most_one_chars(word, &token.chars))
        .then(|| 650 + token_len.min(32) * 4)
}

fn edit_distance_at_most_one_chars(left: &str, right: &[char]) -> bool {
    if left.chars().eq(right.iter().copied()) {
        return true;
    }
    let left_len = left.chars().count();
    let right_len = right.len();
    if left_len.abs_diff(right_len) > 1 {
        return false;
    }

    if left_len == right_len {
        let mut first_mismatch = None;
        let mut second_mismatch = None;
        for (idx, (left_ch, right_ch)) in left.chars().zip(right.iter().copied()).enumerate() {
            if left_ch == right_ch {
                continue;
            }
            if first_mismatch.is_none() {
                first_mismatch = Some((idx, left_ch, right_ch));
            } else if second_mismatch.is_none() {
                second_mismatch = Some((idx, left_ch, right_ch));
            } else {
                return false;
            }
        }
        return match (first_mismatch, second_mismatch) {
            (Some(_), None) => true,
            (
                Some((first_idx, first_left, first_right)),
                Some((second_idx, second_left, second_right)),
            ) => {
                second_idx == first_idx + 1
                    && first_left == second_right
                    && second_left == first_right
            }
            _ => false,
        };
    }

    let left_chars = left.chars().collect::<Vec<_>>();
    let (shorter, longer): (&[char], &[char]) = if left_len < right_len {
        (&left_chars, right)
    } else {
        (right, &left_chars)
    };
    let mut shorter = shorter.iter().peekable();
    let mut longer = longer.iter().peekable();
    let mut skipped = false;
    while let (Some(short_ch), Some(long_ch)) = (shorter.peek(), longer.peek()) {
        if short_ch == long_ch {
            shorter.next();
            longer.next();
        } else if skipped {
            return false;
        } else {
            skipped = true;
            longer.next();
        }
    }
    true
}

#[cfg(test)]
fn edit_distance_at_most_one(left: &str, right: &str) -> bool {
    edit_distance_at_most_one_chars(left, &right.chars().collect::<Vec<_>>())
}

fn fuzzy_token_score(haystack: &str, token: &PreparedToken<'_>) -> Option<usize> {
    let token_len = token.len();
    if token_len < 3 {
        return None;
    }
    let first_token_char = *token.chars.first()?;

    let mut best = None;
    for (start_idx, (byte_pos, ch)) in haystack.char_indices().enumerate() {
        if ch != first_token_char {
            continue;
        }

        let mut token_idx = 1usize;
        let mut end_idx = start_idx;
        let rest = &haystack[byte_pos + ch.len_utf8()..];
        for (offset, candidate) in rest.chars().enumerate() {
            if token.chars.get(token_idx).copied() == Some(candidate) {
                end_idx = start_idx + offset + 1;
                token_idx += 1;
                if token_idx == token_len {
                    break;
                }
            }
        }
        if token_idx != token_len {
            continue;
        }

        let span = end_idx.saturating_sub(start_idx) + 1;
        let gaps = span.saturating_sub(token_len);
        let boundary = if is_text_boundary_before(haystack, byte_pos) {
            60
        } else {
            0
        };
        let compactness = 120usize.saturating_sub(gaps.min(24) * 5);
        let position = 40usize.saturating_sub(start_idx.min(40));
        let score = (250 + token_len.min(32) * 4 + boundary + compactness + position).min(590);
        best = Some(best.map_or(score, |current: usize| current.max(score)));
    }
    best
}

fn context_score(fields: &SearchFields, context: &SearchContext) -> usize {
    let cwd_score = context
        .cwd
        .as_ref()
        .filter(|cwd| fields.cwd == cwd.as_str())
        .map(|_| SAME_CWD_BOOST)
        .unwrap_or(0);
    let hostname_score = context
        .hostname
        .as_ref()
        .filter(|hostname| fields.hostname == hostname.as_str())
        .map(|_| SAME_HOSTNAME_BOOST)
        .unwrap_or(0);
    cwd_score + hostname_score
}

fn normalize_context_value(value: Option<String>) -> Option<String> {
    value
        .map(|value| sanitize_one_line(&value).trim().to_lowercase())
        .filter(|value| !value.is_empty())
}

fn tokenize_query(query: &str) -> Vec<String> {
    split_unescaped(query, ' ', None)
        .into_iter()
        .filter(|token| !token.is_empty())
        .collect()
}

fn split_unescaped(query: &str, separator: char, max_split: Option<usize>) -> Vec<String> {
    if query.is_empty() {
        return Vec::new();
    }

    let chars = query.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut splits = 1usize;
    let mut in_double_quote = false;
    let mut in_single_quote = false;
    let mut idx = 0usize;
    while idx < chars.len() {
        let ch = chars[idx];
        if max_split.is_none_or(|max| splits < max)
            && ch == separator
            && !in_single_quote
            && !in_double_quote
        {
            if !token.is_empty() {
                tokens.push(std::mem::take(&mut token));
            }
            splits += 1;
        } else if ch == '\\' && idx + 1 < chars.len() {
            let next = chars[idx + 1];
            if matches!(next, '-' | ':' | '\\') {
                token.push(ch);
            }
            idx += 1;
            token.push(chars[idx]);
        } else if ch == '"'
            && !in_single_quote
            && !heuristic_ignore_unclosed_quote(in_double_quote, '"', &chars, idx)
        {
            in_double_quote = !in_double_quote;
        } else if ch == '\''
            && !in_double_quote
            && !heuristic_ignore_unclosed_quote(in_single_quote, '\'', &chars, idx)
        {
            in_single_quote = !in_single_quote;
        } else {
            if (in_single_quote || in_double_quote) && separator == ' ' {
                if ch == ':' {
                    token.push('\\');
                }
                if ch == '-' && token.is_empty() {
                    token.push('\\');
                }
            }
            token.push(ch);
        }
        idx += 1;
    }
    tokens.push(token);
    tokens
}

fn heuristic_ignore_unclosed_quote(
    is_currently_in_quoted_string: bool,
    quote_type: char,
    query: &[char],
    idx: usize,
) -> bool {
    if is_currently_in_quoted_string {
        return false;
    }
    !query.iter().skip(idx + 1).any(|ch| *ch == quote_type)
}

fn unescape_query(query: &str) -> String {
    let chars = query.chars().collect::<Vec<_>>();
    let mut out = String::new();
    let mut idx = 0usize;
    while idx < chars.len() {
        if chars[idx] == '\\' {
            idx += 1;
        }
        if let Some(ch) = chars.get(idx) {
            out.push(*ch);
        }
        idx += 1;
    }
    out
}

fn selected_entry<'a>(
    entries: &'a [Entry],
    rows: &[SearchRow],
    filtered: &[usize],
    cursor: usize,
) -> Option<&'a Entry> {
    let row_index = *filtered.get(cursor)?;
    let entry_index = rows.get(row_index)?.entry_index;
    entries.get(entry_index)
}

fn render_query(query: &[char], cursor: usize) -> String {
    let mut out = String::from("\x1b[90m> \x1b[0m");
    for idx in 0..=query.len() {
        if idx == cursor {
            if let Some(ch) = query.get(idx) {
                out.push_str("\x1b[7m");
                out.push(*ch);
                out.push_str("\x1b[0m");
            } else {
                out.push_str("\x1b[7m \x1b[0m");
            }
        }
        if idx < query.len() && idx != cursor {
            out.push(query[idx]);
        }
    }
    out
}

fn table_visible_rows(term_height: usize, show_help: bool) -> usize {
    let reserved = if show_help { 10 } else { 8 };
    term_height
        .saturating_sub(reserved)
        .clamp(3, TABLE_MAX_ROWS)
}

fn compute_column_widths(
    rows: &[SearchRow],
    _filtered: &[usize],
    inside_width: usize,
) -> [usize; 6] {
    let separator_width = COLUMNS.len().saturating_sub(1);
    let available = inside_width.saturating_sub(separator_width);
    let mut widths = std::array::from_fn(|idx| {
        COLUMNS[idx]
            .preferred_width
            .max(display_width(COLUMNS[idx].title))
            .max(COLUMNS[idx].min_width)
    });
    let mut wanted = widths;

    // hishtory처럼 현재 필터 결과가 아니라 넓은 기본 샘플 기준으로 컬럼 anchor를 고정한다.
    for row in rows.iter().take(1000) {
        for (idx, cell) in row.cells.iter().enumerate() {
            wanted[idx] = wanted[idx].max(display_width(cell));
        }
    }

    let mut total: usize = widths.iter().sum();
    while total < available {
        let mut progressed = false;
        for idx in 0..widths.len() {
            let target = if idx == COMMAND_COLUMN {
                wanted[idx].max(widths[idx] + available.saturating_sub(total))
            } else if idx == CWD_COLUMN {
                cwd_flex_target(wanted[idx])
            } else {
                wanted[idx].saturating_add(5)
            };
            if widths[idx] < target {
                widths[idx] += 1;
                total += 1;
                progressed = true;
                if total >= available {
                    break;
                }
            }
        }
        if !progressed {
            widths[COMMAND_COLUMN] += available - total;
            break;
        }
    }

    while total > available {
        let Some(idx) = widest_shrinkable_column(&widths) else {
            break;
        };
        widths[idx] -= 1;
        total -= 1;
    }

    widths
}

fn cwd_flex_target(wanted_width: usize) -> usize {
    let preferred = COLUMNS[CWD_COLUMN]
        .preferred_width
        .max(display_width(COLUMNS[CWD_COLUMN].title))
        .max(COLUMNS[CWD_COLUMN].min_width);
    wanted_width
        .saturating_add(5)
        .saturating_mul(2)
        .checked_div(3)
        .unwrap_or(usize::MAX)
        .max(preferred)
}

fn widest_shrinkable_column(widths: &[usize; 6]) -> Option<usize> {
    widths
        .iter()
        .enumerate()
        .filter(|(idx, width)| **width > COLUMNS[*idx].min_width)
        .max_by_key(|(idx, width)| (**width, *idx == COMMAND_COLUMN || *idx == 1))
        .map(|(idx, _)| idx)
}

fn max_hscroll(
    rows: &[SearchRow],
    filtered: &[usize],
    widths: &[usize; 6],
    scroll_top: usize,
    visible_rows: usize,
) -> usize {
    filtered
        .iter()
        .skip(scroll_top)
        .take(visible_rows.max(1) * 2)
        .filter_map(|idx| rows.get(*idx))
        .flat_map(|row| row.cells.iter().zip(widths.iter()))
        .map(|(cell, width)| display_width(cell).saturating_sub(*width).saturating_add(2))
        .max()
        .unwrap_or(0)
}

fn render_cells(
    cells: &[String],
    widths: &[usize; 6],
    inside_width: usize,
    hscroll: usize,
) -> String {
    let mut content = cells
        .iter()
        .zip(widths.iter())
        .map(|(cell, width)| fit_cell(cell, *width, hscroll))
        .collect::<Vec<_>>()
        .join(" ");

    let width = display_width(&content);
    if width < inside_width {
        content.push_str(&" ".repeat(inside_width - width));
    } else if width > inside_width {
        content = take_display_width(&content, inside_width);
    }
    content
}

fn fit_cell(value: &str, width: usize, hscroll: usize) -> String {
    let len = display_width(value);
    let visible = if hscroll > 0 && len > width {
        let start = hscroll.min(len.saturating_sub(1));
        format!("...{}", skip_display_width(value, start))
    } else {
        value.to_string()
    };
    pad_right(&truncate_right(&visible, width), width)
}

fn truncate_right(value: &str, width: usize) -> String {
    if display_width(value) <= width {
        return value.to_string();
    }
    if width <= 3 {
        return take_display_width(value, width);
    }
    format!("{}...", take_display_width(value, width - 3))
}

fn pad_right(value: &str, width: usize) -> String {
    let mut out = value.to_string();
    let len = display_width(&out);
    if len < width {
        out.push_str(&" ".repeat(width - len));
    }
    out
}

fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

fn char_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
}

fn take_display_width(value: &str, max_width: usize) -> String {
    let mut out = String::new();
    let mut width = 0;
    for ch in value.chars() {
        let ch_width = char_width(ch);
        if ch_width > 0 && width + ch_width > max_width {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out
}

fn skip_display_width(value: &str, columns: usize) -> String {
    if columns == 0 {
        return value.to_string();
    }

    let mut width = 0;
    for (idx, ch) in value.char_indices() {
        let ch_width = char_width(ch);
        if width >= columns {
            return value[idx..].to_string();
        }
        width += ch_width;
    }
    String::new()
}

fn table_line(content: &str, selected: bool) -> String {
    if selected {
        format!("│\x1b[7m{content}\x1b[0m│")
    } else {
        format!("│{content}│")
    }
}

fn top_border(width: usize) -> String {
    format!("┌{}┐", "─".repeat(width.saturating_sub(2)))
}

fn separator_border(width: usize) -> String {
    format!("├{}┤", "─".repeat(width.saturating_sub(2)))
}

fn bottom_border(width: usize) -> String {
    format!("└{}┘", "─".repeat(width.saturating_sub(2)))
}

fn previous_word_boundary(query: &[char], cursor: usize) -> usize {
    let mut last_boundary = 0;
    for boundary in word_boundaries(query) {
        if boundary >= cursor {
            return last_boundary;
        }
        last_boundary = boundary;
    }
    last_boundary
}

fn next_word_boundary(query: &[char], cursor: usize) -> usize {
    word_boundaries(query)
        .into_iter()
        .find(|boundary| *boundary > cursor)
        .unwrap_or(query.len())
}

fn word_boundaries(query: &[char]) -> Vec<usize> {
    let mut boundaries = vec![0];
    let mut prev_was_break = false;
    for (idx, ch) in query.iter().enumerate() {
        if is_word_break(*ch) {
            if !prev_was_break {
                boundaries.push(idx);
            }
            prev_was_break = true;
        } else {
            prev_was_break = false;
        }
    }
    if !prev_was_break {
        boundaries.push(query.len());
    }
    boundaries
}

fn is_word_break(ch: char) -> bool {
    ch == ' ' || ch == '-'
}

fn compact_cwd(cwd: &str) -> String {
    let cwd = sanitize_one_line(cwd);
    if cwd.is_empty() {
        return "-".to_string();
    }

    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        if cwd == home {
            return "~".to_string();
        }
        if let Some(rest) = cwd.strip_prefix(&(home + "/")) {
            return format!("~/{rest}");
        }
    }

    cwd
}

fn format_timestamp(ts: time::OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        ts.year(),
        u8::from(ts.month()),
        ts.day(),
        ts.hour(),
        ts.minute(),
        ts.second()
    )
}

fn format_duration(duration_ms: i64) -> String {
    if duration_ms <= 0 {
        return "N/A".to_string();
    }

    let total_ms = duration_ms as u64;
    if total_ms < 1000 {
        return format!("{total_ms}ms");
    }

    let total_sec = total_ms / 1000;
    let millis = (total_ms % 1000) as u16;
    let seconds = total_sec % 60;
    let minutes = (total_sec / 60) % 60;
    let hours = total_sec / 3600;

    let seconds_part = format_seconds_part(seconds, millis);
    if hours > 0 {
        format!("{hours}h{minutes}m{seconds_part}s")
    } else if minutes > 0 {
        format!("{minutes}m{seconds_part}s")
    } else {
        format!("{seconds_part}s")
    }
}

fn format_seconds_part(seconds: u64, millis: u16) -> String {
    if millis == 0 {
        return seconds.to_string();
    }

    let mut fractional = format!("{millis:03}");
    while fractional.ends_with('0') {
        fractional.pop();
    }
    format!("{seconds}.{fractional}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;

    fn entry(hostname: &str, cwd: &str, cmd: &str) -> Entry {
        Entry {
            entry_id: format!("{hostname}-{cmd}"),
            device_id: "macbook".to_string(),
            user_id: "user1".to_string(),
            ts: OffsetDateTime::from_unix_timestamp(1).unwrap(),
            cmd: cmd.to_string(),
            cwd: cwd.to_string(),
            exit_code: 0,
            duration_ms: 4139,
            shell: "zsh".to_string(),
            hostname: hostname.to_string(),
            version: crate::build_info::VERSION.to_string(),
        }
    }

    fn quality_entry(id: &str, hostname: &str, cwd: &str, cmd: &str) -> Entry {
        let mut entry = entry(hostname, cwd, cmd);
        entry.entry_id = id.to_string();
        entry
    }

    struct SearchQualityCase {
        name: &'static str,
        query: &'static str,
        context: SearchContext,
        expected_id: String,
        expected_command: String,
        entries: Vec<Entry>,
    }

    fn search_quality_case(
        name: &'static str,
        query: &'static str,
        context: SearchContext,
        expected_id: &'static str,
        candidates: &[(&str, &str, &str, &str)],
    ) -> SearchQualityCase {
        let expected_command = candidates
            .iter()
            .find(|(id, _, _, _)| *id == expected_id)
            .map(|(_, _, _, cmd)| (*cmd).to_string())
            .expect("quality case expected candidate must exist");
        SearchQualityCase {
            name,
            query,
            context,
            expected_id: format!("{name}:{expected_id}"),
            expected_command,
            entries: candidates
                .iter()
                .map(|(id, hostname, cwd, cmd)| {
                    quality_entry(&format!("{name}:{id}"), hostname, cwd, cmd)
                })
                .collect(),
        }
    }

    fn search_quality_cases() -> Vec<SearchQualityCase> {
        vec![
            search_quality_case(
                "exact command",
                "git status",
                SearchContext::default(),
                "target",
                &[
                    ("noise-1", "workstation", "/work/app", "git status --short"),
                    ("noise-2", "workstation", "/work/app", "echo git status"),
                    ("target", "workstation", "/work/app", "git status"),
                ],
            ),
            search_quality_case(
                "executable and subcommand prefixes",
                "git reb",
                SearchContext::default(),
                "target",
                &[
                    (
                        "noise-1",
                        "workstation",
                        "/work/app",
                        "echo git repository backup",
                    ),
                    ("noise-2", "workstation", "/work/app", "rg rebase git-notes"),
                    (
                        "target",
                        "workstation",
                        "/work/app",
                        "git rebase --continue",
                    ),
                ],
            ),
            search_quality_case(
                "out of order plain tokens",
                "workspace cargo test",
                SearchContext::default(),
                "target",
                &[
                    (
                        "noise-1",
                        "workstation",
                        "/work/app",
                        "echo workspace cargo build",
                    ),
                    (
                        "noise-2",
                        "workstation",
                        "/work/app",
                        "cargo metadata --workspace",
                    ),
                    (
                        "target",
                        "workstation",
                        "/work/app",
                        "cargo test --workspace",
                    ),
                ],
            ),
            search_quality_case(
                "adjacent transposition typo",
                "kubetcl get pods",
                SearchContext::default(),
                "target",
                &[
                    ("noise-1", "workstation", "/work/app", "echo get pods"),
                    (
                        "noise-2",
                        "workstation",
                        "/work/app",
                        "kubectl describe pod",
                    ),
                    ("target", "workstation", "/work/app", "kubectl get pods"),
                ],
            ),
            search_quality_case(
                "inserted character typo",
                "carggo test",
                SearchContext::default(),
                "target",
                &[
                    ("noise-1", "workstation", "/work/app", "go test ./..."),
                    ("noise-2", "workstation", "/work/app", "cargo build"),
                    (
                        "target",
                        "workstation",
                        "/work/app",
                        "cargo test --workspace",
                    ),
                ],
            ),
            search_quality_case(
                "deleted character typo",
                "pythn manage",
                SearchContext::default(),
                "target",
                &[
                    ("noise-1", "workstation", "/work/app", "manage service"),
                    ("noise-2", "workstation", "/work/app", "python -m pytest"),
                    (
                        "target",
                        "workstation",
                        "/work/app",
                        "python manage.py runserver",
                    ),
                ],
            ),
            search_quality_case(
                "compact fuzzy token",
                "dcmp",
                SearchContext::default(),
                "target",
                &[
                    (
                        "noise-1",
                        "workstation",
                        "/work/app",
                        "deploy cluster migration plan",
                    ),
                    ("noise-2", "workstation", "/work/app", "docker build"),
                    ("target", "workstation", "/work/app", "docker compose ps"),
                ],
            ),
            search_quality_case(
                "command before metadata",
                "docker",
                SearchContext::default(),
                "target",
                &[
                    ("noise-1", "docker-host", "/work/app", "echo ready"),
                    ("noise-2", "workstation", "/work/docker", "ls -lah"),
                    ("target", "workstation", "/work/app", "docker build"),
                ],
            ),
            search_quality_case(
                "current cwd tie break",
                "cargo test",
                SearchContext::new(Some("/work/current".to_string()), None),
                "target",
                &[
                    ("noise-1", "workstation", "/work/other", "cargo test"),
                    ("target", "workstation", "/work/current", "cargo test"),
                    ("noise-2", "workstation", "/work/third", "cargo test"),
                ],
            ),
            search_quality_case(
                "current host tie break",
                "rr sync-status",
                SearchContext::new(None, Some("current-host".to_string())),
                "target",
                &[
                    ("noise-1", "other-host", "/work/app", "rr sync-status"),
                    ("target", "current-host", "/work/app", "rr sync-status"),
                    ("noise-2", "third-host", "/work/app", "rr sync-status"),
                ],
            ),
            search_quality_case(
                "exact phrase before contextual wrapper",
                "kubectl get pods",
                SearchContext::new(Some("/work/current".to_string()), None),
                "target",
                &[
                    (
                        "noise-1",
                        "workstation",
                        "/work/current",
                        "echo kubectl get pods",
                    ),
                    ("target", "workstation", "/work/other", "kubectl get pods"),
                    (
                        "noise-2",
                        "workstation",
                        "/work/current",
                        "kubectl get pods --all-namespaces",
                    ),
                ],
            ),
            search_quality_case(
                "plain tokens across command and cwd",
                "rustory test",
                SearchContext::default(),
                "target",
                &[
                    (
                        "noise-1",
                        "workstation",
                        "/work/other",
                        "echo rustory notes",
                    ),
                    ("noise-2", "workstation", "/work/rustory", "cargo build"),
                    ("target", "workstation", "/work/rustory", "cargo test"),
                ],
            ),
            search_quality_case(
                "unicode plain tokens",
                "배포 상태",
                SearchContext::default(),
                "target",
                &[
                    ("noise-1", "workstation", "/work/app", "echo 배포"),
                    ("noise-2", "workstation", "/work/app", "상태 확인"),
                    ("target", "workstation", "/work/app", "rr 배포 상태 확인"),
                ],
            ),
            search_quality_case(
                "whole token before internal substring",
                "release",
                SearchContext::default(),
                "target",
                &[
                    (
                        "noise-1",
                        "workstation",
                        "/work/app",
                        "echo prerelease-ready",
                    ),
                    ("target", "workstation", "/work/app", "release --dry-run"),
                    ("noise-2", "workstation", "/work/app", "echo release notes"),
                ],
            ),
            search_quality_case(
                "recency for equal relevance",
                "just check",
                SearchContext::default(),
                "target",
                &[
                    ("target", "workstation", "/work/app", "just check"),
                    ("noise-1", "workstation", "/work/app", "just check"),
                    ("noise-2", "workstation", "/work/app", "just check"),
                ],
            ),
            search_quality_case(
                "shell flag as plain token",
                "workspace test",
                SearchContext::default(),
                "target",
                &[
                    ("noise-1", "workstation", "/work/app", "cargo test"),
                    (
                        "noise-2",
                        "workstation",
                        "/work/app",
                        "cargo metadata --workspace",
                    ),
                    (
                        "target",
                        "workstation",
                        "/work/app",
                        "cargo test --workspace",
                    ),
                ],
            ),
        ]
    }

    fn expected_rank_in_entries(
        case: &SearchQualityCase,
        entries: &[Entry],
        query: &str,
    ) -> Option<usize> {
        let rows = build_search_rows(entries);
        filter_rows_with_context(&rows, query, &case.context)
            .iter()
            .position(|row_index| {
                rows.get(*row_index)
                    .and_then(|row| entries.get(row.entry_index))
                    .is_some_and(|entry| entry.entry_id == case.expected_id)
            })
            .map(|rank| rank + 1)
    }

    fn expected_rank(case: &SearchQualityCase, query: &str) -> Option<usize> {
        expected_rank_in_entries(case, &case.entries, query)
    }

    fn expected_command_rank_in_entries(
        case: &SearchQualityCase,
        entries: &[Entry],
        query: &str,
    ) -> Option<usize> {
        let rows = build_search_rows(entries);
        filter_rows_with_context(&rows, query, &case.context)
            .iter()
            .position(|row_index| {
                rows.get(*row_index)
                    .and_then(|row| entries.get(row.entry_index))
                    .is_some_and(|entry| entry.cmd == case.expected_command)
            })
            .map(|rank| rank + 1)
    }

    fn top_entry_ids(case: &SearchQualityCase, entries: &[Entry], query: &str) -> Vec<String> {
        let rows = build_search_rows(entries);
        filter_rows_with_context(&rows, query, &case.context)
            .into_iter()
            .take(3)
            .filter_map(|row_index| {
                rows.get(row_index)
                    .and_then(|row| entries.get(row.entry_index))
                    .map(|entry| entry.entry_id.clone())
            })
            .collect()
    }

    fn keystrokes_to_rank(
        case: &SearchQualityCase,
        entries: &[Entry],
        target_rank: usize,
    ) -> usize {
        let query_chars = case.query.chars().collect::<Vec<_>>();
        for count in 1..=query_chars.len() {
            let prefix = query_chars[..count].iter().collect::<String>();
            if expected_command_rank_in_entries(case, entries, &prefix)
                .is_some_and(|rank| rank <= target_rank)
            {
                return count;
            }
        }
        query_chars.len() + 1
    }

    #[test]
    fn sanitize_one_line_replaces_control_separators() {
        assert_eq!(sanitize_one_line("a\nb\rc\td"), "a b c d");
    }

    #[test]
    fn sanitize_one_line_escapes_terminal_control_sequences() {
        let got = sanitize_one_line("safe\x1b]52;c;AAAA\x07cmd\u{85}");
        assert!(!got.contains('\x1b'));
        assert!(!got.contains('\x07'));
        assert!(!got.contains('\u{85}'));
        assert!(got.contains("\\x1b]52;c;AAAA\\x07cmd\\u{85}"));
    }

    #[test]
    fn search_rows_include_hishtory_style_metadata() {
        let entries = vec![entry(
            "sample-node",
            "/Users/user/code/src/sample-project",
            "cat ./sample-project/docs/README.md",
        )];

        let rows = build_search_rows(&entries);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].search_fields.hostname.contains("sample-node"));
        assert!(rows[0].search_fields.device_id.contains("macbook"));
        assert!(
            rows[0]
                .search_fields
                .cwd
                .contains("/users/user/code/src/sample-project")
        );
        assert!(rows[0].search_fields.compact_cwd.contains("sample-project"));
        assert!(
            rows[0]
                .search_fields
                .command
                .contains("cat ./sample-project/docs/readme.md")
        );
        assert_eq!(rows[0].cells[0], "sample-node");
        assert!(rows[0].cells[2].contains("1970-01-01 00:00:01 UTC"));
        assert_eq!(rows[0].cells[3], "4.139s");
    }

    #[test]
    fn filter_rows_matches_split_fuzzy_metadata_tokens() {
        let entries = vec![
            entry(
                "sample-node",
                "/Users/user/code/src/sample-project",
                "docker compose up docs",
            ),
            entry("node0", "/home/user", "ls -lah"),
        ];
        let rows = build_search_rows(&entries);

        let matches = filter_rows(&rows, "smp pro doc");
        assert_eq!(matches, vec![0]);

        let matches = filter_rows(&rows, "pro doc");
        assert_eq!(matches, vec![0]);
    }

    #[test]
    fn filter_rows_supports_hishtory_style_field_atoms_and_negation() {
        let mut typo = entry("node0", "/home/user", "ifcofnig");
        typo.exit_code = 127;
        let entries = vec![
            entry(
                "macbook",
                "/Users/user/code/src/rustory",
                "cargo test search",
            ),
            entry("node0", "/tmp", "ls -lah"),
            typo,
        ];
        let rows = build_search_rows(&entries);

        assert_eq!(filter_rows(&rows, "cwd:/tmp ls"), vec![1]);
        assert_eq!(filter_rows(&rows, "hostname:node exit_code:127"), vec![2]);
        assert_eq!(filter_rows(&rows, "-exit_code:0"), vec![2]);
        assert_eq!(filter_rows(&rows, "cmd:\"cargo test\""), vec![0]);
    }

    #[test]
    fn filter_rows_ranks_command_matches_above_metadata_matches() {
        let entries = vec![
            entry("docker-host", "/Users/user/code/src/rustory", "echo ok"),
            entry(
                "macbook",
                "/Users/user/code/src/rustory",
                "docker compose up",
            ),
            entry("node0", "/Users/user/docker/context", "ls -lah"),
        ];
        let rows = build_search_rows(&entries);

        let matches = filter_rows(&rows, "docker");

        assert_eq!(matches, vec![1, 2, 0]);
    }

    #[test]
    fn search_quality_corpus_meets_targets() {
        let cases = search_quality_cases();
        let corpus = cases
            .iter()
            .flat_map(|case| case.entries.iter().cloned())
            .collect::<Vec<_>>();
        let mut hit_at_one = 0usize;
        let mut hit_at_three = 0usize;
        let mut reciprocal_rank_sum = 0.0f64;
        let mut top_one_misses = Vec::new();
        let mut top_three_misses = Vec::new();
        let mut top_one_keystrokes = Vec::new();
        let mut top_three_keystrokes = Vec::new();

        for case in &cases {
            let rank = expected_command_rank_in_entries(case, &corpus, case.query);
            if rank == Some(1) {
                hit_at_one += 1;
            } else {
                top_one_misses.push((case.name, rank, top_entry_ids(case, &corpus, case.query)));
            }
            if rank.is_some_and(|rank| rank <= 3) {
                hit_at_three += 1;
            } else {
                top_three_misses.push((case.name, rank));
            }
            if let Some(rank) = rank {
                reciprocal_rank_sum += 1.0 / rank as f64;
            }
            top_one_keystrokes.push(keystrokes_to_rank(case, &corpus, 1));
            top_three_keystrokes.push(keystrokes_to_rank(case, &corpus, 3));
        }

        top_one_keystrokes.sort_unstable();
        top_three_keystrokes.sort_unstable();
        let median_top_one_keystrokes = top_one_keystrokes[top_one_keystrokes.len() / 2];
        let median_top_three_keystrokes = top_three_keystrokes[top_three_keystrokes.len() / 2];
        let mrr = reciprocal_rank_sum / cases.len() as f64;
        eprintln!(
            "search_quality cases={} hit_at_1={} hit_at_3={} mrr={mrr:.3} median_top1_keystrokes={median_top_one_keystrokes} median_top3_keystrokes={median_top_three_keystrokes}",
            cases.len(),
            hit_at_one,
            hit_at_three
        );
        assert!(
            hit_at_one * 100 >= cases.len() * 75,
            "Hit@1 target missed: {hit_at_one}/{}; misses={top_one_misses:?}",
            cases.len()
        );
        assert!(
            hit_at_three * 100 >= cases.len() * 90,
            "Hit@3 target missed: {hit_at_three}/{}; misses={top_three_misses:?}",
            cases.len()
        );
        assert!(mrr >= 0.80, "MRR target missed: {mrr:.3}");
        assert!(
            median_top_one_keystrokes <= 6,
            "median Top-1 keystrokes target missed: {median_top_one_keystrokes}; all={top_one_keystrokes:?}"
        );
        assert!(
            median_top_three_keystrokes <= 5,
            "median Top-3 keystrokes target missed: {median_top_three_keystrokes}; all={top_three_keystrokes:?}"
        );
    }

    #[test]
    fn typo_match_handles_single_edit_and_transposition() {
        assert!(edit_distance_at_most_one("kubectl", "kubetcl"));
        assert!(edit_distance_at_most_one("kubectl", "kubactl"));
        assert!(edit_distance_at_most_one("cargo", "carggo"));
        assert!(edit_distance_at_most_one("python", "pythn"));
        assert!(!edit_distance_at_most_one("kubectl", "kubeadm"));
    }

    #[test]
    fn current_context_breaks_ties_without_overriding_phrase_quality() {
        let cwd_case = &search_quality_cases()[8];
        assert_eq!(expected_rank(cwd_case, cwd_case.query), Some(1));

        let phrase_case = &search_quality_cases()[10];
        assert_eq!(expected_rank(phrase_case, phrase_case.query), Some(1));
    }

    #[test]
    #[ignore = "release-mode 100k-row latency budget"]
    fn search_quality_benchmark_100k_rows() {
        let entries = (0..100_000)
            .map(|idx| {
                quality_entry(
                    &format!("entry-{idx}"),
                    if idx % 5 == 0 { "node-a" } else { "node-b" },
                    &format!("/work/project-{}", idx % 200),
                    match idx % 5 {
                        0 => "cargo test --workspace",
                        1 => "docker compose ps",
                        2 => "kubectl get pods --all-namespaces",
                        3 => "git rebase --continue",
                        _ => "python manage.py runserver",
                    },
                )
            })
            .collect::<Vec<_>>();
        let rows = build_search_rows(&entries);
        let context = SearchContext::new(
            Some("/work/project-42".to_string()),
            Some("node-a".to_string()),
        );
        let queries = [
            "cargo test",
            "docker comp",
            "kubetcl pods",
            "git reb",
            "project-42",
        ];

        for query in queries {
            std::hint::black_box(filter_rows_with_context(&rows, query, &context));
        }

        let mut samples = Vec::new();
        let mut per_query = vec![Vec::new(); queries.len()];
        for _ in 0..5 {
            for (query_idx, query) in queries.iter().enumerate() {
                let started = std::time::Instant::now();
                let matches = filter_rows_with_context(&rows, query, &context);
                std::hint::black_box(matches.len());
                let elapsed = started.elapsed();
                samples.push(elapsed);
                per_query[query_idx].push(elapsed);
            }
        }
        for (query, query_samples) in queries.iter().zip(per_query.iter_mut()) {
            query_samples.sort_unstable();
            let query_p95 = query_samples[query_samples.len() - 1];
            eprintln!(
                "search_quality_benchmark query={query:?} p95_ms={:.2}",
                query_p95.as_secs_f64() * 1000.0
            );
        }
        samples.sort_unstable();
        let p95_index = (samples.len() * 95).div_ceil(100).saturating_sub(1);
        let p95 = samples[p95_index];
        eprintln!(
            "search_quality_benchmark rows={} samples={} p95_ms={:.2}",
            rows.len(),
            samples.len(),
            p95.as_secs_f64() * 1000.0
        );
        assert!(
            p95 <= Duration::from_millis(50),
            "100k-row p95 latency budget exceeded: {p95:?}"
        );
    }

    #[test]
    fn tokenize_query_matches_hishtory_quotes_and_escaping() {
        assert_eq!(
            tokenize_query(r#"cwd:"foo bar :baz\"" docker"#),
            vec![r#"cwd:foo bar \:baz""#.to_string(), "docker".to_string()]
        );
        assert_eq!(
            tokenize_query(r#"ls \-Slah foo\:bar"#),
            vec![
                r#"ls"#.to_string(),
                r#"\-Slah"#.to_string(),
                r#"foo\:bar"#.to_string()
            ]
        );
        assert_eq!(
            parse_search_terms(r#""docker run" hostname:node -exit_code:127"#),
            vec![
                SearchTerm {
                    negate: false,
                    matcher: SearchMatcher::Any("docker run".to_string())
                },
                SearchTerm {
                    negate: false,
                    matcher: SearchMatcher::Field {
                        field: SearchField::Hostname,
                        value: "node".to_string()
                    }
                },
                SearchTerm {
                    negate: true,
                    matcher: SearchMatcher::Field {
                        field: SearchField::ExitCode,
                        value: "127".to_string()
                    }
                }
            ]
        );
    }

    #[test]
    fn table_height_stays_inline_on_tall_terminals() {
        assert_eq!(table_visible_rows(60, false), 20);
        assert!(table_visible_rows(12, false) < 20);
    }

    #[test]
    fn cwd_column_uses_reduced_flex_width_and_leaves_room_for_command() {
        let entries = vec![entry(
            "user.local",
            "/home/user/very/long/project/path/with/many/components/that/should/not/dominate",
            "cargo test --workspace --all-targets --features production-daily-driver-readiness",
        )];
        let rows = build_search_rows(&entries);
        let filtered = [0];
        let widths = compute_column_widths(&rows, &filtered, 220);
        let cwd_wanted = display_width(&rows[0].cells[CWD_COLUMN]);

        assert_eq!(widths[CWD_COLUMN], cwd_flex_target(cwd_wanted));
        assert!(widths[COMMAND_COLUMN] > widths[CWD_COLUMN]);
    }

    #[test]
    fn column_widths_stay_stable_across_query_changes() {
        let entries = vec![
            entry("node0", "/home/user", "which rr"),
            entry(
                "user.local",
                "/Users/user/code/src/rustory",
                "scripts/finalize-and-push.sh --message 'fix: prefer relay for tracker p2p sync'",
            ),
            entry(
                "samplex.local",
                "/opt/homebrew/Library/Taps/veeso/homebrew-termscp",
                "z codex",
            ),
        ];
        let rows = build_search_rows(&entries);
        let which_matches = filter_rows(&rows, "wh");
        let finalize_matches = filter_rows(&rows, "finalize");
        let tap_matches = filter_rows(&rows, "termscp");

        assert_ne!(which_matches, finalize_matches);
        assert_ne!(finalize_matches, tap_matches);
        assert_eq!(
            compute_column_widths(&rows, &which_matches, 220),
            compute_column_widths(&rows, &finalize_matches, 220)
        );
        assert_eq!(
            compute_column_widths(&rows, &which_matches, 220),
            compute_column_widths(&rows, &tap_matches, 220)
        );
    }

    #[test]
    fn render_cell_supports_horizontal_scroll() {
        assert_eq!(fit_cell("abcdefghijklmnopqrstuvwxyz", 10, 0), "abcdefg...");
        assert_eq!(fit_cell("abcdefghijklmnopqrstuvwxyz", 10, 10), "...klmn...");
    }

    #[test]
    fn render_cell_uses_terminal_display_width_for_wide_text() {
        assert_eq!(display_width("잠 동 사니"), 10);
        assert_eq!(
            fit_cell("잠 동 사니/container-manager", 12, 0),
            "잠 동 사... "
        );
        assert_eq!(
            display_width(&fit_cell("잠 동 사니/container-manager", 12, 0)),
            12
        );
    }

    #[test]
    fn render_cells_never_exceeds_inside_width_with_wide_text() {
        let cells = [
            "user.local".to_string(),
            "~/SynologyDrive/잠 동 사니/container-manager".to_string(),
            "2026-06-28 05:31:35 UTC".to_string(),
            "N/A".to_string(),
            "0".to_string(),
            "exit".to_string(),
        ];
        let widths = [10, 20, 23, 8, 9, 18];
        let rendered = render_cells(&cells, &widths, 93, 0);
        assert_eq!(display_width(&rendered), 93);
    }

    #[test]
    fn render_uses_tty_line_endings_for_raw_mode() {
        let frame = render_frame_lines(&["Search Query: > ".to_string(), "table".to_string()]);
        assert_eq!(frame, "Search Query: > \r\ntable\r\n");
        assert!(!frame.contains(" \ntable"));
    }

    #[test]
    fn first_render_starts_below_shell_prompt() {
        assert_eq!(redraw_prefix(0), "\r\n");
        assert_eq!(redraw_prefix(12), "\r\x1b[12F\x1b[J");
    }

    #[test]
    fn finish_clear_erases_last_rendered_frame() {
        assert_eq!(clear_rendered_frame(0), "");
        assert_eq!(clear_rendered_frame(23), "\r\x1b[24F\x1b[J");
    }

    #[test]
    fn parse_escape_sequence_supports_table_scroll_keys() {
        assert_eq!(parse_escape_sequence(b"[1;2D"), Key::ShiftLeft);
        assert_eq!(parse_escape_sequence(b"[1;2C"), Key::ShiftRight);
        assert_eq!(parse_escape_sequence(b"[1;5D"), Key::CtrlLeft);
        assert_eq!(parse_escape_sequence(b"[1;5C"), Key::CtrlRight);
    }

    #[test]
    fn parse_escape_sequence_supports_sgr_mouse_wheel() {
        assert_eq!(parse_escape_sequence(b"[<64;12;5M"), Key::MouseWheelUp);
        assert_eq!(parse_escape_sequence(b"[<65;120;42M"), Key::MouseWheelDown);
        assert_eq!(parse_escape_sequence(b"[<68;12;5M"), Key::MouseWheelUp);
        assert_eq!(parse_escape_sequence(b"[<69;12;5M"), Key::MouseWheelDown);
        assert_eq!(parse_escape_sequence(b"[<64;12;5m"), Key::Unknown);
    }

    #[test]
    fn parse_plain_key_maps_ctrl_k_to_delete_selected() {
        assert_eq!(parse_plain_key(0x0b), Key::DeleteSelected);
    }

    #[test]
    fn utf8_multibyte_keys_build_korean_and_emoji_query() {
        let mut query = Vec::new();
        for value in ["한", "글", "😀"] {
            match parse_utf8_key(value.as_bytes()) {
                Key::Char(ch) => query.push(ch),
                key => panic!("expected character key for {value:?}, got {key:?}"),
            }
        }

        let query = query.iter().collect::<String>();
        assert_eq!(query, "한글😀");
        let rows = build_search_rows(&[entry("host", "/tmp", "echo 한글😀")]);
        assert_eq!(filter_rows(&rows, &query), vec![0]);
        assert_eq!(utf8_scalar_len("한".as_bytes()[0]), Some(3));
        assert_eq!(utf8_scalar_len("😀".as_bytes()[0]), Some(4));
    }

    #[test]
    fn utf8_key_decoder_rejects_invalid_or_control_scalars() {
        assert_eq!(parse_utf8_key(&[0xe3, 0x28, 0xa1]), Key::Unknown);
        assert_eq!(parse_utf8_key("\u{85}".as_bytes()), Key::Unknown);
        assert_eq!(utf8_scalar_len(0x80), None);
        assert_eq!(utf8_scalar_len(0xc0), None);
        assert_eq!(utf8_scalar_len(0xf5), None);
    }

    #[test]
    fn selected_entry_uses_filtered_cursor() {
        let entries = vec![
            entry("node0", "/home/user", "which rr"),
            entry(
                "macbook",
                "/Users/user/code/src/rustory",
                "cargo test search",
            ),
        ];
        let rows = build_search_rows(&entries);
        let filtered = filter_rows(&rows, "cargo");

        let got = selected_entry(&entries, &rows, &filtered, 0).unwrap();

        assert_eq!(got.entry_id, entries[1].entry_id);
        assert_eq!(got.cmd, "cargo test search");
    }

    #[test]
    fn filter_rows_excluding_deleted_hides_deleted_entries() {
        let entries = vec![
            entry("node0", "/home/user", "which rr"),
            entry(
                "macbook",
                "/Users/user/code/src/rustory",
                "cargo test search",
            ),
        ];
        let rows = build_search_rows(&entries);
        let deleted = HashSet::from([entries[1].entry_id.clone()]);

        let got = filter_rows_excluding_deleted(&entries, &rows, "cargo", &deleted);

        assert!(got.is_empty());
    }

    #[test]
    fn word_boundaries_match_hishtory_style_breaks() {
        let query = "cargo test --workspace".chars().collect::<Vec<_>>();
        assert_eq!(previous_word_boundary(&query, query.len()), 10);
        assert_eq!(next_word_boundary(&query, 0), 5);
    }

    #[test]
    fn format_duration_matches_human_shell_history_shape() {
        assert_eq!(format_duration(0), "N/A");
        assert_eq!(format_duration(919), "919ms");
        assert_eq!(format_duration(2180), "2.18s");
        assert_eq!(format_duration(291694), "4m51.694s");
    }
}
