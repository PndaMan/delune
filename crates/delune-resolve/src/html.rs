//! Just enough HTML reading to pull metadata out of public pages.
//!
//! Services without open APIs still describe their pages for link previews: Open
//! Graph `<meta>` tags, JSON-LD `<script>` blocks, or an embedded JSON payload. These
//! helpers find those pieces by scanning tags; they don't build a DOM, and they don't
//! need to, because the markup we read is machine-generated and regular.

/// The `content` of the first `<meta>` whose `property` or `name` is `key`.
pub fn meta(html: &str, key: &str) -> Option<String> {
    tags(html, "meta").find_map(|tag| {
        let named = attr(tag, "property").or_else(|| attr(tag, "name"))?;
        if named.eq_ignore_ascii_case(key) { attr(tag, "content").map(|c| decode_entities(&c)) } else { None }
    })
}

/// Bodies of every `<script>` whose opening tag has `attribute="value"`.
pub fn scripts<'a>(html: &'a str, attribute: &'a str, value: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    let mut rest = html;
    std::iter::from_fn(move || {
        loop {
            let start = find_ignore_case(rest, "<script")?;
            let after = &rest[start..];
            let open_end = after.find('>')?;
            let tag = &after[..open_end];
            let body_start = open_end + 1;
            let body_len = find_ignore_case(&after[body_start..], "</script")?;
            let body = &after[body_start..body_start + body_len];
            rest = &after[body_start + body_len..];
            if attr(tag, attribute).is_some_and(|v| v.eq_ignore_ascii_case(value)) {
                return Some(body.trim());
            }
        }
    })
}

/// Opening tags named `name`, each as the text between `<name` and `>`.
fn tags<'a>(html: &'a str, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    let needle = format!("<{name}");
    let mut rest = html;
    std::iter::from_fn(move || {
        let start = find_ignore_case(rest, &needle)?;
        let after = &rest[start + needle.len()..];
        let end = after.find('>')?;
        rest = &after[end..];
        Some(&after[..end])
    })
}

/// The value of `name="…"` or `name='…'` inside an opening tag.
fn attr(tag: &str, name: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut from = 0;
    while let Some(offset) = find_ignore_case(&tag[from..], name) {
        let at = from + offset;
        from = at + name.len();
        // Whole attribute names only: `content` must not match inside `data-content`.
        let boundary = at == 0 || bytes[at - 1].is_ascii_whitespace();
        let rest = tag[from..].trim_start();
        if !boundary || !rest.starts_with('=') {
            continue;
        }
        let value = rest[1..].trim_start();
        let quote = value.chars().next()?;
        if quote == '"' || quote == '\'' {
            let inner = &value[1..];
            return inner.find(quote).map(|end| inner[..end].to_owned());
        }
        let end = value.find(|c: char| c.is_ascii_whitespace() || c == '/').unwrap_or(value.len());
        return Some(value[..end].to_owned());
    }
    None
}

fn find_ignore_case(haystack: &str, needle: &str) -> Option<usize> {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

/// Decode the entities that show up in page titles: `&amp;`, `&#39;`, `&#x27;` and friends.
pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let decoded = rest.find(';').filter(|&end| end <= 10).and_then(|end| {
            let entity = &rest[1..end];
            let ch = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .map_or_else(
                        || entity.strip_prefix('#').and_then(|d| d.parse::<u32>().ok()),
                        |hex| u32::from_str_radix(hex, 16).ok(),
                    )
                    .and_then(char::from_u32),
            };
            ch.map(|c| (c, end))
        });
        if let Some((c, end)) = decoded {
            out.push(c);
            rest = &rest[end + 1..];
        } else {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

/// Seconds in an ISO 8601 duration: `PT4M35S`, or Bandcamp's `P00H05M12S`.
pub fn iso_duration_secs(s: &str) -> Option<u32> {
    let body = s.strip_prefix('P')?;
    let (mut total, mut number) = (0u32, String::new());
    for c in body.chars() {
        match c {
            'T' => {}
            '0'..='9' => number.push(c),
            'H' | 'M' | 'S' => {
                let value: u32 = number.parse().ok()?;
                number.clear();
                let unit = match c {
                    'H' => 3600,
                    'M' => 60,
                    _ => 1,
                };
                total += value * unit;
            }
            _ => return None,
        }
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_meta_tags_in_any_attribute_order() {
        let html = r#"<head><meta property="og:title" content="OK Computer, Radiohead - Qobuz">
            <META content='Sea Change &amp; more' name="og:description"/>
            <meta data-content="decoy" property="og:type" content="music.album"></head>"#;
        assert_eq!(meta(html, "og:title").as_deref(), Some("OK Computer, Radiohead - Qobuz"));
        assert_eq!(meta(html, "og:description").as_deref(), Some("Sea Change & more"));
        assert_eq!(meta(html, "og:type").as_deref(), Some("music.album"));
        assert_eq!(meta(html, "og:image"), None);
    }

    #[test]
    fn finds_scripts_by_attribute() {
        let html = r#"<script>var a = 1</script>
            <script type="application/ld+json">{"name":"Tomorrow's Harvest"}</script>
            <script id="__NEXT_DATA__" type="application/json">{"props":{}}</script>"#;
        assert_eq!(
            scripts(html, "type", "application/ld+json").collect::<Vec<_>>(),
            [r#"{"name":"Tomorrow's Harvest"}"#]
        );
        assert_eq!(scripts(html, "id", "__NEXT_DATA__").next(), Some(r#"{"props":{}}"#));
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(decode_entities("Guns N&#39; Roses &amp; Friends &#x2014; Live"), "Guns N' Roses & Friends — Live");
        assert_eq!(decode_entities("AT&T & co"), "AT&T & co");
    }

    #[test]
    fn parses_iso_durations() {
        assert_eq!(iso_duration_secs("PT4M35S"), Some(275));
        assert_eq!(iso_duration_secs("P00H05M12S"), Some(312));
        assert_eq!(iso_duration_secs("PT1H2S"), Some(3602));
        assert_eq!(iso_duration_secs("4:35"), None);
    }
}
