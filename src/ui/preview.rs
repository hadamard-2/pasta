use crate::*;
use chrono::{DateTime, Local};

pub(crate) fn masked_secret_preview(content: &str) -> String {
    let width = content.chars().count().clamp(8, 32);
    "•".repeat(width)
}

/// The list-row form of a masked secret: a short leading hint followed by a
/// fixed run of dots. The preview pane can afford to hide a secret completely,
/// but a results list can't — a column of identical dot runs gives the user no
/// way to tell one secret from another, so the row trades a few leading
/// characters for that. The dot run is fixed width on purpose, so the row
/// doesn't also leak how long the value is.
pub(crate) fn partially_masked_secret_title(content: &str) -> String {
    const HINT_CHARS: usize = 4;
    const MASK_CHARS: usize = 8;

    let mask = "•".repeat(MASK_CHARS);
    // Below twice the hint length there isn't enough left to hide, so hide it all.
    if content.chars().count() <= HINT_CHARS * 2 {
        return mask;
    }

    let hint: String = content.chars().take(HINT_CHARS).collect();
    format!("{hint}{mask}")
}

pub(crate) fn preview_content(content: &str) -> String {
    let wrapped = expanded_preview_content(content);
    let mut lines = wrapped.lines();
    let preview: Vec<&str> = lines.by_ref().take(PREVIEW_LINE_LIMIT).collect();
    let has_more = lines.next().is_some();

    if has_more {
        let mut joined = preview.join("\n");
        joined.push('…');
        joined
    } else {
        preview.join("\n")
    }
}

pub(crate) fn expanded_preview_content(content: &str) -> String {
    let normalized = content.replace('\r', "").replace('\t', "    ");
    wrap_long_words(&normalized, PREVIEW_WRAP_RUN)
}

pub(crate) fn bounded_preview_content(content: &str, limit: usize) -> (String, bool) {
    if content.len() <= limit {
        return (content.to_owned(), false);
    }

    let mut end = limit.min(content.len());
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }

    let mut bounded = content[..end].to_owned();
    bounded.push_str("\n\n... Preview shortened for speed.");
    (bounded, true)
}

fn wrap_long_words(input: &str, max_run: usize) -> String {
    let mut out = String::with_capacity(input.len() + (input.len() / max_run.max(1)));
    let mut run = 0_usize;

    for ch in input.chars() {
        out.push(ch);
        if ch == '\n' || ch.is_whitespace() {
            run = 0;
            continue;
        }

        run += 1;
        if run >= max_run {
            out.push('\n');
            run = 0;
        }
    }

    out
}

pub(crate) fn format_timestamp(timestamp: &str) -> String {
    timestamp
        .split('T')
        .nth(1)
        .and_then(|time| time.get(0..5))
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "now".to_owned())
}

pub(crate) fn format_timestamp_detail(timestamp: &str) -> String {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|value| {
            value
                .with_timezone(&Local)
                .format("%b %-d, %Y • %-I:%M %p")
                .to_string()
        })
        .unwrap_or_else(|_| timestamp.to_owned())
}

pub(crate) fn format_byte_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;
    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }
    if unit_index == 0 {
        format!("{bytes} {}", UNITS[unit_index])
    } else {
        format!("{size:.1} {}", UNITS[unit_index])
    }
}

pub(crate) fn format_image_metadata(image: &ImageAttachment) -> String {
    format!(
        "{}×{} · {}",
        image.width,
        image.height,
        format_byte_size(image.byte_size)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_mask_keeps_secrets_distinguishable_without_showing_them() {
        let first = partially_masked_secret_title("ghp_ZkQ1r8Tn4wLm2xVb");
        let second = partially_masked_secret_title("sk-live_9dPq3RtY7hNs");

        assert_ne!(
            first, second,
            "two secrets must not render as the same row label"
        );
        assert!(first.starts_with("ghp_"));
        assert!(!first.contains("ZkQ1r8Tn4wLm2xVb"));
        assert_eq!(
            first.chars().count(),
            second.chars().count(),
            "a fixed-width mask keeps the row from leaking the secret's length"
        );
    }

    #[test]
    fn partial_mask_hides_short_secrets_entirely() {
        // Below twice the hint width, a hint would give away most of the value.
        let masked = partially_masked_secret_title("hunter2!");

        assert!(masked.chars().all(|ch| ch == '\u{2022}'));
    }
}
