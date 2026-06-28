use crate::core::Entry;
use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

const FZF_HEADER: &str =
    "Hostname       CWD                    Timestamp               Runtime Exit Code Command";
const HOST_WIDTH: usize = 14;
const CWD_WIDTH: usize = 22;
const TIMESTAMP_WIDTH: usize = 23;
const RUNTIME_WIDTH: usize = 8;
const EXIT_CODE_WIDTH: usize = 9;
const DISPLAY_ROW_MIN_WIDTH: usize = 240;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct FzfCapabilities {
    highlight_line: bool,
}

pub fn select_command(entries: &[Entry]) -> Result<Option<String>> {
    if entries.is_empty() {
        return Ok(None);
    }

    let lines = format_fzf_lines(entries);
    let Some(selected_line) = run_fzf(&lines)? else {
        return Ok(None);
    };

    Ok(selected_command_from_line(entries, &selected_line)
        .or_else(|| parse_legacy_selected_cmd(&selected_line)))
}

fn format_fzf_lines(entries: &[Entry]) -> Vec<String> {
    entries
        .iter()
        .map(|e| {
            format!(
                "{}\t{}\t{}",
                sanitize_one_line(&e.entry_id),
                format_search_text(e),
                format_display_row(e)
            )
        })
        .collect()
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

fn format_display_row(entry: &Entry) -> String {
    let row = format!(
        "{host} {cwd} {timestamp:<TIMESTAMP_WIDTH$} {runtime:>RUNTIME_WIDTH$} {exit_code:>EXIT_CODE_WIDTH$} {cmd}",
        host = fit_tail(&entry.hostname, HOST_WIDTH),
        cwd = fit_tail(&compact_cwd(&entry.cwd), CWD_WIDTH),
        timestamp = format_timestamp(entry.ts),
        runtime = format_duration(entry.duration_ms),
        exit_code = entry.exit_code,
        cmd = sanitize_one_line(&entry.cmd),
    );
    pad_display_row(row, DISPLAY_ROW_MIN_WIDTH)
}

fn pad_display_row(mut row: String, min_width: usize) -> String {
    let len = row.chars().count();
    if len < min_width {
        row.push_str(&" ".repeat(min_width - len));
    }
    row
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

fn fit_tail(value: &str, width: usize) -> String {
    let value = sanitize_one_line(value);
    if value.chars().count() <= width {
        return format!("{value:<width$}");
    }

    if width <= 3 {
        return value.chars().take(width).collect();
    }

    let keep = width - 3;
    let mut tail = value.chars().rev().take(keep).collect::<Vec<_>>();
    tail.reverse();
    format!("...{}", tail.into_iter().collect::<String>())
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

fn run_fzf(lines: &[String]) -> Result<Option<String>> {
    let capabilities = detect_fzf_capabilities();
    let args = fzf_args(capabilities);
    let mut child = Command::new("fzf")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!("fzf not found (install fzf and ensure it's in PATH)")
            } else {
                anyhow::anyhow!("spawn fzf: {err}")
            }
        })?;

    {
        let mut stdin = child.stdin.take().context("open fzf stdin")?;
        for line in lines {
            stdin
                .write_all(line.as_bytes())
                .with_context(|| format!("write fzf stdin: {line:?}"))?;
            stdin.write_all(b"\n").context("write fzf stdin newline")?;
        }
        // drop stdin to signal EOF
    }

    let out = child.wait_with_output().context("wait fzf")?;

    // fzf exit code:
    // - 0: selection made
    // - 1: no match
    // - 130: interrupted (ESC/C-c)
    match out.status.code() {
        Some(0) => {
            let selected = String::from_utf8_lossy(&out.stdout);
            let selected = selected.trim_end_matches(['\n', '\r']).to_string();
            if selected.is_empty() {
                Ok(None)
            } else {
                Ok(Some(selected))
            }
        }
        Some(1) | Some(130) => Ok(None),
        Some(code) => anyhow::bail!("fzf exited with status code {code}"),
        None => anyhow::bail!("fzf terminated by signal"),
    }
}

fn detect_fzf_capabilities() -> FzfCapabilities {
    let Ok(output) = Command::new("fzf").arg("--help").output() else {
        return FzfCapabilities::default();
    };

    let mut help = String::from_utf8_lossy(&output.stdout).into_owned();
    help.push_str(&String::from_utf8_lossy(&output.stderr));

    FzfCapabilities {
        highlight_line: help.contains("--highlight-line"),
    }
}

fn fzf_args(capabilities: FzfCapabilities) -> Vec<&'static str> {
    let mut args = vec![
        "--no-sort",
        "--height=~100%",
        "--layout=reverse",
        "--border=sharp",
        "--info=hidden",
        "--no-separator",
        "--no-hscroll",
        "--pointer= ",
        "--marker= ",
        "--prompt=Search Query: > ",
        "--header",
        FZF_HEADER,
        "--delimiter=\t",
        "--with-nth=3..",
        "--tiebreak=index",
    ];
    if capabilities.highlight_line {
        args.push("--highlight-line");
    }
    args
}

fn selected_command_from_line(entries: &[Entry], selected_line: &str) -> Option<String> {
    let entry_id = parse_selected_entry_id(selected_line)?;
    entries
        .iter()
        .find(|entry| entry.entry_id == entry_id)
        .map(|entry| entry.cmd.clone())
}

fn parse_selected_entry_id(selected_line: &str) -> Option<&str> {
    let line = selected_line.trim_end_matches(['\n', '\r']);
    let (entry_id, _) = line.split_once('\t')?;
    (!entry_id.is_empty()).then_some(entry_id)
}

fn parse_legacy_selected_cmd(selected_line: &str) -> Option<String> {
    let line = selected_line.trim_end_matches(['\n', '\r']);
    if line.is_empty() {
        return None;
    }

    let mut parts = line.splitn(2, '\t');
    let _id = parts.next();
    let cmd = parts.next().unwrap_or(line);
    if cmd.is_empty() {
        None
    } else {
        Some(cmd.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;

    #[test]
    fn sanitize_one_line_replaces_control_separators() {
        assert_eq!(sanitize_one_line("a\nb\rc\td"), "a b c d");
    }

    #[test]
    fn format_fzf_lines_include_hishtory_style_search_metadata() {
        let entries = vec![Entry {
            entry_id: "id-1".to_string(),
            device_id: "macbook".to_string(),
            user_id: "user1".to_string(),
            ts: OffsetDateTime::from_unix_timestamp(1).unwrap(),
            cmd: "cat ./sample-project/docs/README.md".to_string(),
            cwd: "/Users/user/code/src/sample-project".to_string(),
            exit_code: 0,
            duration_ms: 4139,
            shell: "zsh".to_string(),
            hostname: "sample-node".to_string(),
            version: crate::build_info::VERSION.to_string(),
        }];

        let lines = format_fzf_lines(&entries);
        assert_eq!(lines.len(), 1);

        let fields = lines[0].split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0], "id-1");
        assert!(fields[1].contains("sample-node"));
        assert!(fields[1].contains("macbook"));
        assert!(fields[1].contains("/Users/user/code/src/sample-project"));
        assert!(fields[1].contains("cat ./sample-project/docs/README.md"));
        assert!(fields[1].contains("4.139s"));
        assert!(fields[2].contains("sample-node"));
        assert!(fields[2].contains("1970-01-01 00:00:01 UTC"));
        assert!(fields[2].contains("4.139s"));
        assert!(fields[2].contains("cat ./sample-project/docs/README.md"));
        assert!(fields[2].chars().count() >= DISPLAY_ROW_MIN_WIDTH);
    }

    #[test]
    fn pad_display_row_extends_short_rows_for_legacy_fzf_highlight() {
        let row = pad_display_row("host cwd cmd".to_string(), 16);
        assert_eq!(row, "host cwd cmd    ");
        assert_eq!(
            pad_display_row("host cwd command".to_string(), 8),
            "host cwd command"
        );
    }

    #[test]
    fn fzf_args_use_inline_hishtory_like_layout() {
        let args = fzf_args(FzfCapabilities {
            highlight_line: false,
        });
        assert!(args.contains(&"--height=~100%"));
        assert!(args.contains(&"--border=sharp"));
        assert!(args.contains(&"--info=hidden"));
        assert!(args.contains(&"--no-separator"));
        assert!(args.contains(&"--no-hscroll"));
        assert!(args.contains(&"--pointer= "));
        assert!(args.contains(&"--marker= "));
        assert!(!args.contains(&"--highlight-line"));
    }

    #[test]
    fn fzf_args_enable_full_row_highlight_when_supported() {
        let args = fzf_args(FzfCapabilities {
            highlight_line: true,
        });
        assert!(args.contains(&"--highlight-line"));
    }

    #[test]
    fn selected_command_from_line_uses_entry_id_to_preserve_original_command() {
        let entries = vec![Entry {
            entry_id: "id-1".to_string(),
            device_id: "dev1".to_string(),
            user_id: "user1".to_string(),
            ts: OffsetDateTime::from_unix_timestamp(1).unwrap(),
            cmd: "printf 'a\tb'".to_string(),
            cwd: "/tmp".to_string(),
            exit_code: 0,
            duration_ms: 12,
            shell: "zsh".to_string(),
            hostname: "host".to_string(),
            version: crate::build_info::VERSION.to_string(),
        }];

        let selected = "id-1\thost /tmp printf a b\thost /tmp printf a b";
        assert_eq!(
            selected_command_from_line(&entries, selected),
            Some("printf 'a\tb'".to_string())
        );
    }

    #[test]
    fn parse_selected_entry_id_extracts_first_field() {
        assert_eq!(
            parse_selected_entry_id("id-1\tsearch text\tdisplay row"),
            Some("id-1")
        );
    }

    #[test]
    fn parse_legacy_selected_cmd_extracts_cmd_after_tab() {
        assert_eq!(
            parse_legacy_selected_cmd("id-1\techo 1"),
            Some("echo 1".to_string())
        );
    }

    #[test]
    fn parse_legacy_selected_cmd_accepts_plain_line() {
        assert_eq!(
            parse_legacy_selected_cmd("echo 1"),
            Some("echo 1".to_string())
        );
    }

    #[test]
    fn format_duration_matches_human_shell_history_shape() {
        assert_eq!(format_duration(0), "N/A");
        assert_eq!(format_duration(919), "919ms");
        assert_eq!(format_duration(2180), "2.18s");
        assert_eq!(format_duration(291694), "4m51.694s");
    }
}
