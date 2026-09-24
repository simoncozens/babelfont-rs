/// The inverse of decode_sfd_line_escapes, for fields FontForge stores on a
/// single SFD line. A lone CR (name records use it as a line break) becomes
/// "\\n" too.
pub(crate) fn escape_sfd_line(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\\n")
}

pub(crate) fn escape_quoted(s: &str) -> String {
    s.replace('"', "'")
}

pub(crate) fn fmt_num(n: f64) -> String {
    if (n.round() - n).abs() < 1e-6 {
        format!("{}", n.round() as i64)
    } else {
        let s = format!("{:.6}", n);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

pub(crate) fn sanitize_unquoted(s: &str) -> String {
    s.chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect::<String>()
}

/// Decode FontForge's single-line escapes: "\\n" is a line break and "\\\\"
/// a backslash; any other backslash sequence is literal text.
pub(crate) fn decode_sfd_line_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Format a list of glyph names as a feature file glyph group.
/// Single glyph -> just the glyph name; multiple -> [glyph1 glyph2 ...]
/// Escape a string for a quoted Windows-platform FEA name: anything outside
/// printable ASCII, plus the quote and backslash, becomes a `\XXXX` escape of
/// its UTF-16 code units.
pub(crate) fn fea_string_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if (' '..='~').contains(&c) && c != '"' && c != '\\' {
            out.push(c);
        } else {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\{unit:04X}"));
            }
        }
    }
    out
}

/// Split an SFD line on whitespace, keeping quoted spans (quotes included)
/// together.
pub(crate) fn tokenize_preserving_quotes(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for ch in s.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                cur.push(ch);
            }
            c if c.is_whitespace() && !in_quotes => {
                if !cur.is_empty() {
                    out.push(cur.clone());
                    cur.clear();
                }
            }
            _ => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}
