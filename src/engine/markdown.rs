#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Blank,
    Comment,
    Table,
    Heading(u8),
    Bullet,
    Numbered(u32),
    Task(bool),
    Quote,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Classified<'a> {
    pub form: Form,
    pub body: &'a str,
    pub comment_at: Option<usize>,
}

pub fn classify(line: &str) -> Classified<'_> {
    let comment_at = find_comment(line);
    let code = match comment_at {
        Some(i) => &line[..i],
        None => line,
    };
    let trimmed = code.trim();
    if trimmed.is_empty() {
        return Classified {
            form: if comment_at.is_some() {
                Form::Comment
            } else {
                Form::Blank
            },
            body: "",
            comment_at,
        };
    }
    if trimmed.starts_with('|') {
        return Classified {
            form: Form::Table,
            body: "",
            comment_at,
        };
    }
    let (form, body) = peel(trimmed);
    Classified {
        form,
        body: body.trim(),
        comment_at,
    }
}

pub fn math_body(line: &str) -> &str {
    let c = classify(line);
    match c.form {
        Form::Blank | Form::Comment | Form::Table => "",
        _ => c.body,
    }
}

fn peel(trimmed: &str) -> (Form, &str) {
    let mut form = None;
    let mut rest = trimmed;

    for _ in 0..8 {
        let Some((next, after)) = peel_one(rest) else {
            break;
        };
        if form.is_none() {
            form = Some(next);
        }
        rest = after.trim();
    }
    (form.unwrap_or(Form::Text), rest)
}

fn peel_one(s: &str) -> Option<(Form, &str)> {
    if s.starts_with('#') {
        let n = s.bytes().take_while(|b| *b == b'#').count();
        let body = s[n..]
            .strip_prefix(char::is_whitespace)
            .map(str::trim_start)
            .filter(|text| !text.is_empty());
        if let Some(body) = body.filter(|_| (1..=6).contains(&n)) {
            return Some((Form::Heading(n as u8), body));
        }
        return None;
    }
    if let Some(task) = task(s) {
        return Some(task);
    }
    if let Some(rest) = bullet_rest(s) {
        return Some((Form::Bullet, rest));
    }
    if let Some((n, rest)) = numbered(s) {
        return Some((Form::Numbered(n), rest));
    }
    if let Some(rest) = s.strip_prefix('>') {
        return Some((Form::Quote, rest));
    }
    None
}

fn bullet_rest(s: &str) -> Option<&str> {
    let rest = s.strip_prefix('-').or_else(|| s.strip_prefix('*'))?;
    if rest.starts_with(' ') || rest.starts_with('\t') {
        Some(rest)
    } else {
        None
    }
}

fn task(s: &str) -> Option<(Form, &str)> {
    let rest = bullet_rest(s)?.trim_start();
    let rest = rest.strip_prefix('[')?;
    let mut chars = rest.chars();
    let mark = chars.next()?;
    if !matches!(mark, ' ' | 'x' | 'X') {
        return None;
    }
    let after = &rest[mark.len_utf8()..];
    let after = after.strip_prefix(']')?;
    if !after.is_empty() && !after.starts_with(char::is_whitespace) {
        return None;
    }
    Some((Form::Task(matches!(mark, 'x' | 'X')), after))
}

fn numbered(s: &str) -> Option<(u32, &str)> {
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 6 {
        return None;
    }
    let after_digits = &s[digits..];
    let rest = after_digits.strip_prefix('.')?;
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let n = s[..digits].parse().ok()?;
    Some((n, rest))
}

fn find_comment(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut seen = false;
    let mut in_quote = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_quote {
            if c == b'"' {
                in_quote = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_quote = true;
            seen = true;
            i += 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            return Some(i);
        }
        if c == b'#' {
            if !seen {
                while i < bytes.len() && bytes[i] == b'#' {
                    i += 1;
                }
                seen = true;
                continue;
            }
            return Some(i);
        }
        seen = true;

        i += utf8_len(c);
    }
    None
}

fn utf8_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first < 0xE0 {
        2
    } else if first < 0xF0 {
        3
    } else {
        4
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Emphasis {
    pub bold: Vec<(usize, usize)>,
    pub italic: Vec<(usize, usize)>,
    pub markers: Vec<(usize, usize)>,
}

pub fn emphasis(line: &str) -> Emphasis {
    let end = find_comment(line).unwrap_or(line.len());
    let bytes = line.as_bytes();
    let mut out = Emphasis::default();
    let mut i = 0;
    while i < end {
        if bytes[i] == b'`'
            && let Some(close) = find_byte(bytes, i + 1, end, b'`')
        {
            i = close + 1;
            continue;
        }
        if let Some((marker, len)) = opener(bytes, i, end)
            && let Some(close) = find_marker(bytes, i + len, end, marker)
        {
            let inner_start = i + len;
            let inner_end = close;
            let inner = &line[inner_start..inner_end];
            if emphasis_inner(inner) {
                let span = (inner_start, inner_end);
                if len == 2 {
                    out.bold.push(span);
                } else if italic_ok(line, i, close + 1, marker) {
                    out.italic.push(span);
                } else {
                    i += 1;
                    continue;
                }
                out.markers.push((i, i + len));
                out.markers.push((close, close + len));
                i = close + len;
                continue;
            }
        }
        i += utf8_len(bytes[i]);
    }
    out
}

fn opener(bytes: &[u8], i: usize, end: usize) -> Option<(&'static [u8], usize)> {
    if i + 1 < end && bytes[i] == b'*' && bytes[i + 1] == b'*' {
        return Some((b"**", 2));
    }
    if i + 1 < end && bytes[i] == b'_' && bytes[i + 1] == b'_' {
        return Some((b"__", 2));
    }
    if bytes[i] == b'*' {
        return Some((b"*", 1));
    }
    if bytes[i] == b'_' {
        return Some((b"_", 1));
    }
    None
}

fn find_marker(bytes: &[u8], from: usize, end: usize, marker: &[u8]) -> Option<usize> {
    let mut i = from;
    while i + marker.len() <= end {
        if &bytes[i..i + marker.len()] == marker {
            if marker == b"*" && i + 1 < end && bytes[i + 1] == b'*' {
                i += 2;
                continue;
            }
            if marker == b"_" && i + 1 < end && bytes[i + 1] == b'_' {
                i += 2;
                continue;
            }
            return Some(i);
        }
        i += utf8_len(bytes[i]);
    }
    None
}

fn find_byte(bytes: &[u8], from: usize, end: usize, want: u8) -> Option<usize> {
    bytes[from..end]
        .iter()
        .position(|b| *b == want)
        .map(|p| from + p)
}

fn emphasis_inner(inner: &str) -> bool {
    !inner.is_empty()
        && !inner.starts_with(char::is_whitespace)
        && !inner.ends_with(char::is_whitespace)
}

fn italic_ok(line: &str, open: usize, after_close: usize, marker: &[u8]) -> bool {
    if marker == b"_" || marker == b"*" {
        let before = line[..open].chars().next_back();
        let after = line[after_close..].chars().next();
        let glued = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        if glued(before) || glued(after) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_and_comments() {
        let c = classify("- hotel = 3 nights * $180");
        assert_eq!(c.form, Form::Bullet);
        assert_eq!(c.body, "hotel = 3 nights * $180");

        let c = classify("- [x] packed");
        assert_eq!(c.form, Form::Task(true));
        assert_eq!(c.body, "packed");

        let c = classify("- [ ] later");
        assert_eq!(c.form, Form::Task(false));
        assert_eq!(c.body, "later");

        let c = classify("## Week 1");
        assert_eq!(c.form, Form::Heading(2));
        assert_eq!(c.body, "Week 1");

        let c = classify("1. taxi = $12");
        assert_eq!(c.form, Form::Numbered(1));
        assert_eq!(c.body, "taxi = $12");

        let c = classify("> tip = 10% of $40");
        assert_eq!(c.form, Form::Quote);
        assert_eq!(c.body, "tip = 10% of $40");

        let c = classify("> - hotel = $5");
        assert_eq!(c.form, Form::Quote);
        assert_eq!(c.body, "hotel = $5");

        let c = classify("rent = $100 # monthly");
        assert_eq!(c.body, "rent = $100");
        assert!(c.comment_at.is_some());

        let c = classify("# Trip # draft");
        assert_eq!(c.form, Form::Heading(1));
        assert_eq!(c.body, "Trip");

        let c = classify("#ff8800 in rgb");
        assert_eq!(c.form, Form::Text);
        assert_eq!(c.body, "#ff8800 in rgb");
        assert!(c.comment_at.is_none());

        let c = classify("#ff8800 brand orange");
        assert_eq!(c.form, Form::Text);
        assert_eq!(c.body, "#ff8800 brand orange");
        assert!(c.comment_at.is_none());

        let c = classify("#roadtrip plans");
        assert_eq!(c.form, Form::Text);
        assert_eq!(c.body, "#roadtrip plans");

        let c = classify("#ff release notes");
        assert_eq!(c.form, Form::Text);
        assert_eq!(c.body, "#ff release notes");

        let c = classify("#ff8800");
        assert_eq!(c.form, Form::Text);
        assert_eq!(c.body, "#ff8800");

        let c = classify("## f8800 goals");
        assert_eq!(c.form, Form::Heading(2));
        assert_eq!(c.body, "f8800 goals");

        let c = classify("### packing list");
        assert_eq!(c.form, Form::Heading(3));
        assert_eq!(c.body, "packing list");

        let c = classify("#");
        assert_eq!(c.form, Form::Text);
        assert_eq!(c.body, "#");

        let c = classify("####### deep");
        assert_eq!(c.form, Form::Text);
        assert_eq!(c.body, "####### deep");

        let c = classify("\"say # hi\" 1 + 1");
        assert!(c.comment_at.is_none());
        assert_eq!(c.body, "\"say # hi\" 1 + 1");

        let c = classify("| nights | rate |");
        assert_eq!(c.form, Form::Table);

        let c = classify("-$2.50");
        assert_eq!(c.form, Form::Text);
        assert_eq!(c.body, "-$2.50");

        let c = classify("// a note");
        assert_eq!(c.form, Form::Comment);
    }

    #[test]
    fn bold_does_not_eat_multiplication() {
        let e = emphasis("2 * 3");
        assert!(e.italic.is_empty());
        assert!(e.bold.is_empty());
        let e = emphasis("a **bold** word and *one* more");
        assert_eq!(e.bold, vec![(4, 8)]);
        assert_eq!(e.italic, vec![(21, 24)]);
        let e = emphasis("hotel_name = 1");
        assert!(e.italic.is_empty());
    }
}
