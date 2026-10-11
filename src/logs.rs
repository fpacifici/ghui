//! Cleaning of raw GitHub Actions job logs for display.

/// Splits a raw job log into display lines: drops the BOM, the per-line
/// ISO timestamp, ANSI escape sequences, carriage-return progress output
/// and `##[endgroup]` markers.
pub fn clean(raw: &str) -> Vec<String> {
    raw.trim_start_matches('\u{feff}')
        .lines()
        .map(|line| {
            // Progress bars redraw with `\r`; keep the final state.
            let line = line.rsplit('\r').next().unwrap_or(line);
            strip_ansi(strip_timestamp(line))
        })
        .filter(|line| line != "##[endgroup]")
        .collect()
}

/// Index of the first `##[error]` line.
pub fn first_error(lines: &[String]) -> Option<usize> {
    lines.iter().position(|l| l.starts_with("##[error]"))
}

/// Removes a leading `2026-10-10T06:35:39.1781614Z ` timestamp.
fn strip_timestamp(line: &str) -> &str {
    let bytes = line.as_bytes();
    let looks_like_date = bytes.len() > 20
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T';
    if !looks_like_date {
        return line;
    }
    match line.find("Z ") {
        Some(end) if end < 40 => &line[end + 2..],
        _ if line.ends_with('Z') && line.len() < 40 => "",
        _ => line,
    }
}

/// Removes CSI sequences (`ESC [ … final`) and any other stray escapes.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            // Parameters and intermediates, then one final byte in @..~.
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_bom_timestamps_and_ansi() {
        let raw = "\u{feff}2026-10-10T06:35:39.1781614Z Current runner version: '2.337.0'\n\
                   2026-10-10T06:35:40.0000000Z \u{1b}[36;1mecho hi\u{1b}[0m\n\
                   2026-10-10T06:35:41.0000000Z\n\
                   no timestamp here\n";
        assert_eq!(
            clean(raw),
            [
                "Current runner version: '2.337.0'",
                "echo hi",
                "",
                "no timestamp here"
            ]
        );
    }

    #[test]
    fn keeps_the_last_carriage_return_segment_and_drops_endgroup() {
        let raw = "2026-10-10T06:35:39.1Z ##[group]Run tests\n\
                   2026-10-10T06:35:39.2Z 10%\r50%\r100%\n\
                   2026-10-10T06:35:39.3Z ##[endgroup]\n";
        assert_eq!(clean(raw), ["##[group]Run tests", "100%"]);
    }

    #[test]
    fn finds_the_first_error() {
        let lines: Vec<String> = ["ok", "##[error]boom", "##[error]again"]
            .map(String::from)
            .to_vec();
        assert_eq!(first_error(&lines), Some(1));
        assert_eq!(first_error(&lines[..1]), None);
    }
}
