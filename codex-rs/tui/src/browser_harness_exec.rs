//! Human-readable summaries for `browser-harness` heredoc commands.
//!
//! The embedded agent drives the browser by piping a Python script into
//! `./bin/browser-harness <<'PY' … PY`. Rendered raw, those cells are a wall of
//! script text. Since we know the browser-harness helper set, we parse the
//! script body and render a compact list of the browser actions instead.

use ratatui::text::Line;
use ratatui::text::Span;

/// Map a browser-harness helper call to a short human label. `arg` is the raw
/// text inside the first parentheses (may be empty).
fn action_label(func: &str, arg: &str) -> Option<String> {
    let host = || first_url_host(arg);
    let s = match func {
        "new_tab" => match host() {
            Some(h) => format!("open {h}"),
            None => "open tab".to_string(),
        },
        "goto_url" => match host() {
            Some(h) => format!("go to {h}"),
            None => "navigate".to_string(),
        },
        "click_at_xy" => {
            let coords = arg.split(',').take(2).map(str::trim).collect::<Vec<_>>();
            if coords.len() == 2 {
                format!("click ({}, {})", coords[0], coords[1])
            } else {
                "click".to_string()
            }
        }
        "type_text" | "fill_input" => "type text".to_string(),
        "press_key" => {
            let key = unquote(arg.split(',').next().unwrap_or("").trim());
            if key.is_empty() {
                "press key".to_string()
            } else {
                format!("press {key}")
            }
        }
        "scroll" => "scroll".to_string(),
        "capture_screenshot" => "screenshot".to_string(),
        "page_info" => "read page".to_string(),
        "list_tabs" => "list tabs".to_string(),
        "current_tab" => "current tab".to_string(),
        "switch_tab" => "switch tab".to_string(),
        "close_tab" => "close tab".to_string(),
        "new_tab_group" => "open tabs".to_string(),
        "ensure_real_tab" => "attach browser".to_string(),
        "wait_for_load" => "wait for load".to_string(),
        "wait_for_network_idle" => "wait for network".to_string(),
        "wait_for_element" => "wait for element".to_string(),
        "wait" => "wait".to_string(),
        "js" => "run JS".to_string(),
        "upload_file" => "upload file".to_string(),
        "http_get" => "http get".to_string(),
        "secret" | "totp" | "available_secrets" => "use secret".to_string(),
        _ => return None,
    };
    Some(s)
}

/// Return a compact browser-action summary for a `browser-harness` heredoc
/// command display, or `None` if it isn't one.
pub(crate) fn summary_line(cmd_display: &str) -> Option<Line<'static>> {
    let body = heredoc_body(cmd_display)?;
    let mut actions: Vec<String> = Vec::new();
    for raw in body.lines() {
        let mut line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Unwrap `print(<call>)` so `print(page_info())` still reports the call.
        if let Some(inner) = line.strip_prefix("print(").and_then(|s| s.strip_suffix(')')) {
            line = inner.trim();
        }
        if let Some((func, arg)) = parse_call(line) {
            if let Some(label) = action_label(func, arg) {
                // Collapse immediate duplicates (e.g. repeated waits).
                if actions.last().map(String::as_str) != Some(label.as_str()) {
                    actions.push(label);
                }
            }
        }
    }
    if actions.is_empty() {
        return None;
    }
    const MAX: usize = 5;
    let truncated = actions.len() > MAX;
    let shown = actions
        .into_iter()
        .take(MAX)
        .collect::<Vec<_>>()
        .join(", ");
    let mut spans = vec![
        Span::styled("browser", crate::theme::accent()),
        Span::from("  "),
        Span::from(shown),
    ];
    if truncated {
        spans.push(Span::styled(" …", crate::theme::dim()));
    }
    Some(Line::from(spans))
}

/// Whether a command display is a `browser-harness` heredoc (used to suppress
/// its raw stdout dict in the transcript).
pub(crate) fn is_browser_harness_command(cmd_display: &str) -> bool {
    heredoc_body(cmd_display).is_some_and(|body| {
        body.lines().any(|l| parse_call(l.trim()).is_some())
    })
}

/// Extract the body of a `browser-harness <<'TAG' … TAG` heredoc.
fn heredoc_body(cmd: &str) -> Option<&str> {
    if !cmd.contains("browser-harness") {
        return None;
    }
    let marker = cmd.find("<<")?;
    let after = &cmd[marker + 2..];
    // Tag is the token after <<, optionally quoted: 'PY' | "PY" | PY.
    let tag_raw = after.trim_start();
    let (tag, rest) = split_heredoc_tag(tag_raw)?;
    let start = rest.find('\n')? + 1;
    let body_and_tail = &rest[start..];
    // Body ends at a line that is exactly the tag.
    let end = body_and_tail
        .lines()
        .scan(0usize, |offset, line| {
            let at = *offset;
            *offset += line.len() + 1;
            Some((at, line))
        })
        .find(|(_, line)| line.trim() == tag)
        .map(|(at, _)| at)
        .unwrap_or(body_and_tail.len());
    Some(&body_and_tail[..end])
}

fn split_heredoc_tag(s: &str) -> Option<(String, &str)> {
    let bytes = s.as_bytes();
    let (quote, inner) = match bytes.first() {
        Some(b'\'') => (Some(b'\''), &s[1..]),
        Some(b'"') => (Some(b'"'), &s[1..]),
        _ => (None, s),
    };
    match quote {
        Some(q) => {
            let end = inner.find(q as char)?;
            Some((inner[..end].to_string(), &inner[end + 1..]))
        }
        None => {
            let end = inner
                .find(|c: char| c.is_whitespace())
                .unwrap_or(inner.len());
            Some((inner[..end].to_string(), &inner[end..]))
        }
    }
}

/// Parse a `func(args...)` call, allowing a leading `name =` assignment and
/// `await`/`print(` wrappers. Returns `(func, first-paren-contents)`.
fn parse_call(line: &str) -> Option<(&str, &str)> {
    let mut expr = line;
    if let Some((_, rhs)) = expr.split_once('=') {
        // Only treat as assignment when the LHS looks like a bare identifier.
        let lhs = line.split_once('=').map(|(l, _)| l.trim()).unwrap_or("");
        if !lhs.is_empty()
            && lhs
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == ',' || c == ' ')
            && !lhs.contains("==")
        {
            expr = rhs.trim();
        }
    }
    let open = expr.find('(')?;
    let func = expr[..open].trim().rsplit(['.', ' ']).next()?.trim();
    if func.is_empty() || !func.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let close = expr.rfind(')').unwrap_or(expr.len());
    let arg = if close > open { &expr[open + 1..close] } else { "" };
    Some((func, arg))
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches(['\'', '"']).to_string()
}

/// Pull the host out of the first quoted URL in `arg`.
fn first_url_host(arg: &str) -> Option<String> {
    let start = arg.find("http")?;
    let rest = &arg[start..];
    let end = rest
        .find(['\'', '"', ' ', ')'])
        .unwrap_or(rest.len());
    let url = &rest[..end];
    let after_scheme = url.split("://").nth(1)?;
    let host = after_scheme.split('/').next()?;
    Some(host.trim_start_matches("www.").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line<'static>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn summarizes_a_browser_harness_heredoc() {
        let cmd = "./bin/browser-harness <<'PY'\nnew_tab('https://www.amazon.com')\nwait_for_load()\ncapture_screenshot('x.png')\nprint(page_info())\nPY";
        let line = summary_line(cmd).expect("summary");
        let t = text(&line);
        assert!(t.contains("browser"));
        assert!(t.contains("open amazon.com"), "{t}");
        assert!(t.contains("wait for load"), "{t}");
        assert!(t.contains("screenshot"), "{t}");
        assert!(t.contains("read page"), "{t}");
    }

    #[test]
    fn click_coordinates_are_shown() {
        let cmd = "browser-harness <<'PY'\nclick_at_xy(435, 811)\nPY";
        let t = text(&summary_line(cmd).unwrap());
        assert!(t.contains("click (435, 811)"), "{t}");
    }

    #[test]
    fn non_browser_harness_command_is_ignored() {
        assert!(summary_line("ls -la /tmp").is_none());
        assert!(summary_line("git status").is_none());
    }
}
