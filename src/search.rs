use crate::core::Entry;
use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

pub fn select_command(entries: &[Entry]) -> Result<Option<String>> {
    if entries.is_empty() {
        return Ok(None);
    }

    let lines = format_fzf_lines(entries);
    let Some(selected_line) = run_fzf(&lines)? else {
        return Ok(None);
    };

    Ok(parse_selected_cmd(&selected_line))
}

fn format_fzf_lines(entries: &[Entry]) -> Vec<String> {
    entries
        .iter()
        .map(|e| format!("{}\t{}", e.entry_id, sanitize_one_line(&e.cmd)))
        .collect()
}

fn sanitize_one_line(value: &str) -> String {
    value.replace(['\n', '\r'], " ")
}

fn run_fzf(lines: &[String]) -> Result<Option<String>> {
    let mut child = Command::new("fzf")
        .args([
            "--no-sort",
            "--with-nth=2..",
            "--delimiter=\t",
            "--tiebreak=index",
        ])
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

fn parse_selected_cmd(selected_line: &str) -> Option<String> {
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
    fn sanitize_one_line_replaces_newlines() {
        assert_eq!(sanitize_one_line("a\nb\rc"), "a b c");
    }

    #[test]
    fn format_fzf_lines_prefixes_entry_id_and_tab() {
        let entries = vec![Entry {
            entry_id: "id-1".to_string(),
            device_id: "dev1".to_string(),
            user_id: "user1".to_string(),
            ts: OffsetDateTime::from_unix_timestamp(1).unwrap(),
            cmd: "echo 1".to_string(),
            cwd: "/tmp".to_string(),
            exit_code: 0,
            duration_ms: 12,
            shell: "zsh".to_string(),
            hostname: "host".to_string(),
            version: "0.1.0".to_string(),
        }];

        let lines = format_fzf_lines(&entries);
        assert_eq!(lines, vec!["id-1\techo 1".to_string()]);
    }

    #[test]
    fn parse_selected_cmd_extracts_cmd_after_tab() {
        assert_eq!(
            parse_selected_cmd("id-1\techo 1"),
            Some("echo 1".to_string())
        );
    }

    #[test]
    fn parse_selected_cmd_accepts_plain_line() {
        assert_eq!(parse_selected_cmd("echo 1"), Some("echo 1".to_string()));
    }
}
