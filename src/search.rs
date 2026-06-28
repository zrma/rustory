use crate::core::Entry;
use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::time::Duration;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const TABLE_MAX_ROWS: usize = 20;
const TABLE_SCROLL_STEP: usize = 10;
const MIN_FRAME_WIDTH: usize = 40;
const FALLBACK_TERM_WIDTH: usize = 100;
const FALLBACK_TERM_HEIGHT: usize = 30;
const CWD_COLUMN: usize = 1;
const COMMAND_COLUMN: usize = 5;
const FOOTER: &str = "rustory: Search your shell history  • ctrl+h help";
const TTY_LINE_ENDING: &str = "\r\n";

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
    search_text_lower: String,
    cells: [String; 6],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Char(char),
    Backspace,
    Delete,
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
    Home,
    End,
    Unknown,
}

pub fn select_command(entries: &[Entry]) -> Result<Option<String>> {
    if entries.is_empty() {
        return Ok(None);
    }

    let rows = build_search_rows(entries);
    let mut tui = SearchTui::new(entries, rows)?;
    tui.run()
}

fn build_search_rows(entries: &[Entry]) -> Vec<SearchRow> {
    entries
        .iter()
        .enumerate()
        .map(|(entry_index, entry)| {
            let search_text = format_search_text(entry);
            SearchRow {
                entry_index,
                search_text_lower: search_text.to_lowercase(),
                cells: [
                    sanitize_one_line(&entry.hostname),
                    compact_cwd(&entry.cwd),
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
}

impl<'a> SearchTui<'a> {
    fn new(entries: &'a [Entry], rows: Vec<SearchRow>) -> Result<Self> {
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
        };
        tui.refresh_matches(true);
        Ok(tui)
    }

    fn run(&mut self) -> Result<Option<String>> {
        self.render()?;
        loop {
            match self.tty.read_key().context("read search TUI key")? {
                Key::Enter => {
                    self.finish()?;
                    return Ok(self.selected_command());
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
        self.tty.write_all(b"\x1b[?25h\x1b[0m")?;
        self.tty.flush()?;
        self.last_rendered_lines = 0;
        Ok(())
    }

    fn selected_command(&self) -> Option<String> {
        let row_index = *self.filtered.get(self.cursor)?;
        let entry_index = self.rows.get(row_index)?.entry_index;
        Some(self.entries.get(entry_index)?.cmd.clone())
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
            Key::Enter | Key::Quit | Key::Unknown => {}
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
        self.filtered = filter_rows(&self.rows, &query);
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
            lines.push("↑/↓ scroll  pgup/pgdn page  enter select  esc exit".to_string());
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
        let file = OpenOptions::new()
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

        Ok(Self { file, original })
    }

    fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    fn read_key(&mut self) -> std::io::Result<Key> {
        let byte = self.read_byte_blocking()?;
        Ok(match byte {
            b'\r' | b'\n' => Key::Enter,
            0x01 => Key::Home,
            0x03 | 0x04 => Key::Quit,
            0x05 => Key::End,
            0x08 => Key::Help,
            0x0e => Key::Down,
            0x10 => Key::Up,
            0x1b => self.read_escape_sequence()?,
            0x7f => Key::Backspace,
            byte if byte.is_ascii_graphic() || byte == b' ' => Key::Char(byte as char),
            _ => Key::Unknown,
        })
    }

    fn read_escape_sequence(&mut self) -> std::io::Result<Key> {
        let Some(first) = self.read_byte_with_timeout(Duration::from_millis(30))? else {
            return Ok(Key::Quit);
        };
        let mut bytes = vec![first];
        while bytes.len() < 12 {
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
        let _ = self.file.write_all(b"\x1b[?25h\x1b[0m");
        let _ = self.file.flush();
        unsafe {
            libc::tcsetattr(self.fd(), libc::TCSANOW, &self.original);
        }
    }
}

fn parse_escape_sequence(bytes: &[u8]) -> Key {
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

fn filter_rows(rows: &[SearchRow], query: &str) -> Vec<usize> {
    let tokens = query
        .split_whitespace()
        .map(str::to_lowercase)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();

    rows.iter()
        .enumerate()
        .filter_map(|(idx, row)| {
            tokens
                .iter()
                .all(|token| fuzzy_token_matches(&row.search_text_lower, token))
                .then_some(idx)
        })
        .collect()
}

fn fuzzy_token_matches(haystack: &str, token: &str) -> bool {
    if token.is_empty() || haystack.contains(token) {
        return true;
    }

    let mut needle = token.chars();
    let mut wanted = needle.next();
    for ch in haystack.chars() {
        if Some(ch) == wanted {
            wanted = needle.next();
            if wanted.is_none() {
                return true;
            }
        }
    }
    wanted.is_none()
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
    filtered: &[usize],
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

    for row_index in filtered.iter().take(1000).copied() {
        let Some(row) = rows.get(row_index) else {
            continue;
        };
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

fn sanitize_one_line(value: &str) -> String {
    value.replace(['\n', '\r', '\t'], " ")
}

fn format_search_text(entry: &Entry) -> String {
    sanitize_one_line(&format!(
        "{} {} {} {} {} {} {} {}",
        entry.hostname,
        entry.device_id,
        entry.cwd,
        compact_cwd(&entry.cwd),
        format_timestamp(entry.ts),
        format_duration(entry.duration_ms),
        entry.exit_code,
        entry.cmd
    ))
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

    #[test]
    fn sanitize_one_line_replaces_control_separators() {
        assert_eq!(sanitize_one_line("a\nb\rc\td"), "a b c d");
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
        assert!(rows[0].search_text_lower.contains("sample-node"));
        assert!(rows[0].search_text_lower.contains("macbook"));
        assert!(
            rows[0]
                .search_text_lower
                .contains("/users/user/code/src/sample-project")
        );
        assert!(
            rows[0]
                .search_text_lower
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
