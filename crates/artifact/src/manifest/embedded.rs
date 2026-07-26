//! Capability domains declared inside the verified entry document.
//!
//! Signed `requires` tags stay authoritative. Napplets published without them
//! still declare their domains in the `napplet-requires` meta element of
//! `/index.html`, and those bytes are pinned by the signed path digest and the
//! signed aggregate, so they carry exactly the authority of the tags. Reading
//! them lets such builds reach permission review instead of launching with an
//! empty capability inventory.
//!
//! The scan is deliberately narrow: only `<meta>` elements in the document head
//! are considered, raw-text and comment regions are skipped so a string literal
//! inside a bundled script cannot forge a declaration, and only names already
//! inside the pinned compatibility inventory survive.

use super::KNOWN_REQUIREMENTS;

const REQUIRES_META_NAME: &[u8] = b"napplet-requires";
const CONFIG_SCHEMA_META_NAME: &[u8] = b"napplet-config-schema";
/// Generous enough for the largest declaration the trusted shell will read
/// back out of the same head, so no legal element truncates the scan.
const MAXIMUM_ELEMENT_BYTES: usize = 256 * 1024;

/// Extracts the capability domains declared by `<meta name="napplet-requires">`
/// in the head of one verified entry document, in declaration order.
///
/// Unknown or malformed names are ignored rather than refused: this path only
/// proposes a permission review, and a bounded subset is always safer than
/// refusing to install a build whose signed tags already verified.
pub fn embedded_requirements(document: &[u8]) -> Vec<&'static str> {
    let mut domains: Vec<&'static str> = Vec::new();
    let Some(content) = head_meta_content(document, REQUIRES_META_NAME) else {
        return domains;
    };
    for field in content.split(',') {
        if domains.len() == KNOWN_REQUIREMENTS.len() {
            break;
        }
        let field = field.trim();
        let Some(known) = KNOWN_REQUIREMENTS
            .iter()
            .find(|known| field.eq_ignore_ascii_case(known))
        else {
            continue;
        };
        if !domains.contains(known) {
            domains.push(known);
        }
    }
    domains
}

/// Extracts the raw JSON text declared by `<meta name="napplet-config-schema">`
/// in the head of one verified entry document. The caller parses and validates
/// it; this only undoes the HTML attribute escaping.
pub fn embedded_config_schema(document: &[u8]) -> Option<String> {
    head_meta_content(document, CONFIG_SCHEMA_META_NAME)
}

/// Returns the unescaped `content` of the first head `<meta>` with this name.
fn head_meta_content(document: &[u8], meta_name: &[u8]) -> Option<String> {
    let mut cursor = 0usize;
    while cursor < document.len() {
        let open = next_tag(document, cursor)?;
        if starts_with_ignoring_case(&document[open..], b"<!--") {
            cursor = skip_until(document, open + 4, b"-->")?;
            continue;
        }
        let Some(name_end) = tag_name_end(document, open) else {
            cursor = open + 1;
            continue;
        };
        let name = &document[open + 1..name_end];
        if equals_ignoring_case(name, b"body") || equals_ignoring_case(name, b"/head") {
            return None;
        }
        let close = element_end(document, name_end)?;
        if equals_ignoring_case(name, b"meta")
            && let Some(content) = meta_content(&document[name_end..close], meta_name)
        {
            return Some(content);
        }
        cursor = close + 1;
        if is_raw_text(name) {
            let mut terminator = Vec::with_capacity(name.len() + 2);
            terminator.extend_from_slice(b"</");
            terminator.extend_from_slice(name);
            cursor = skip_until(document, cursor, &terminator)?;
        }
    }
    None
}

fn meta_content(attributes: &[u8], meta_name: &[u8]) -> Option<String> {
    let mut matches_name = false;
    let mut content: Option<&[u8]> = None;
    let mut cursor = 0usize;
    while let Some((name, value, next)) = next_attribute(attributes, cursor) {
        if equals_ignoring_case(name, b"name") {
            matches_name = equals_ignoring_case(value, meta_name);
        } else if equals_ignoring_case(name, b"content") {
            content = Some(value);
        }
        cursor = next;
    }
    matches_name
        .then_some(content)
        .flatten()
        .map(unescape_attribute)
}

/// Undoes the attribute escaping every HTML serializer emits. Unrecognized
/// references are kept verbatim so nothing is silently rewritten.
fn unescape_attribute(value: &[u8]) -> String {
    let text = String::from_utf8_lossy(value);
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_ref();
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        match &rest[1..end] {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            reference => match numeric_reference(reference) {
                Some(character) => out.push(character),
                None => {
                    out.push('&');
                    rest = &rest[1..];
                    continue;
                }
            },
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn numeric_reference(reference: &str) -> Option<char> {
    let digits = reference.strip_prefix('#')?;
    let code = match digits.strip_prefix(['x', 'X']) {
        Some(hexadecimal) => u32::from_str_radix(hexadecimal, 16).ok()?,
        None => digits.parse::<u32>().ok()?,
    };
    char::from_u32(code)
}

/// Reads one `name="value"` pair, returning the offset just past it.
fn next_attribute(attributes: &[u8], from: usize) -> Option<(&[u8], &[u8], usize)> {
    let mut cursor = from;
    while cursor < attributes.len()
        && (attributes[cursor].is_ascii_whitespace() || attributes[cursor] == b'/')
    {
        cursor += 1;
    }
    let start = cursor;
    while cursor < attributes.len()
        && !attributes[cursor].is_ascii_whitespace()
        && attributes[cursor] != b'='
        && attributes[cursor] != b'/'
    {
        cursor += 1;
    }
    if cursor == start {
        return None;
    }
    let name = &attributes[start..cursor];
    while cursor < attributes.len() && attributes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    if cursor >= attributes.len() || attributes[cursor] != b'=' {
        return Some((name, &[], cursor));
    }
    cursor += 1;
    while cursor < attributes.len() && attributes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    if cursor >= attributes.len() {
        return Some((name, &[], cursor));
    }
    let quote = attributes[cursor];
    if quote == b'"' || quote == b'\'' {
        cursor += 1;
        let start = cursor;
        while cursor < attributes.len() && attributes[cursor] != quote {
            cursor += 1;
        }
        let value = &attributes[start..cursor];
        return Some((name, value, (cursor + 1).min(attributes.len())));
    }
    let start = cursor;
    while cursor < attributes.len() && !attributes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    Some((name, &attributes[start..cursor], cursor))
}

fn next_tag(document: &[u8], from: usize) -> Option<usize> {
    document[from..]
        .iter()
        .position(|byte| *byte == b'<')
        .map(|offset| from + offset)
}

fn tag_name_end(document: &[u8], open: usize) -> Option<usize> {
    let mut cursor = open + 1;
    if cursor < document.len() && document[cursor] == b'/' {
        cursor += 1;
    }
    let start = cursor;
    while cursor < document.len()
        && (document[cursor].is_ascii_alphanumeric() || document[cursor] == b'-')
    {
        cursor += 1;
    }
    (cursor > start).then_some(cursor)
}

/// Finds the `>` that closes one element, ignoring `>` inside quoted values.
fn element_end(document: &[u8], from: usize) -> Option<usize> {
    let limit = from
        .saturating_add(MAXIMUM_ELEMENT_BYTES)
        .min(document.len());
    let mut cursor = from;
    let mut quote: Option<u8> = None;
    while cursor < limit {
        let byte = document[cursor];
        match quote {
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None if byte == b'"' || byte == b'\'' => quote = Some(byte),
            None if byte == b'>' => return Some(cursor),
            None => {}
        }
        cursor += 1;
    }
    None
}

fn skip_until(document: &[u8], from: usize, terminator: &[u8]) -> Option<usize> {
    let mut cursor = from;
    while cursor + terminator.len() <= document.len() {
        if equals_ignoring_case(&document[cursor..cursor + terminator.len()], terminator) {
            return Some(cursor + terminator.len());
        }
        cursor += 1;
    }
    None
}

fn is_raw_text(name: &[u8]) -> bool {
    equals_ignoring_case(name, b"script")
        || equals_ignoring_case(name, b"style")
        || equals_ignoring_case(name, b"textarea")
        || equals_ignoring_case(name, b"title")
}

fn starts_with_ignoring_case(value: &[u8], prefix: &[u8]) -> bool {
    value.len() >= prefix.len() && equals_ignoring_case(&value[..prefix.len()], prefix)
}

fn equals_ignoring_case(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
}
