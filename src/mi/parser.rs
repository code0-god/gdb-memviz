use crate::mi::models::{
    BreakpointInfo, Endian, LocalVar, MiStatus, MiSymbolFileGroup, MiSymbolInfoVariables,
    MiSymbolVariable, StoppedLocation,
};
use regex::Regex;

pub(crate) fn parse_status(line: &str) -> MiStatus {
    if line.starts_with("^done") {
        MiStatus::Done
    } else if line.starts_with("^running") {
        MiStatus::Running
    } else if line.starts_with("^error") {
        let msg = parse_msg_field(line).unwrap_or_else(|| line.to_string());
        MiStatus::Error(msg)
    } else {
        MiStatus::Other(line.to_string())
    }
}

pub(crate) fn parse_msg_field(s: &str) -> Option<String> {
    Regex::new(r#"msg="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(s).map(|c| c[1].to_string()))
}

pub(crate) fn parse_value_field(s: &str) -> Option<String> {
    // Handles escaped quotes/newlines in MI `value="..."`.
    Regex::new(r#"value="((?:\\.|[^"])*)""#)
        .ok()
        .and_then(|re| re.captures(s).map(|c| unescape_value(&c[1])))
}

pub(crate) fn parse_type_field(s: &str) -> Option<String> {
    Regex::new(r#"type="((?:\\.|[^"])*)""#)
        .ok()
        .and_then(|re| re.captures(s).map(|c| unescape_value(&c[1])))
}

pub(crate) fn parse_addr_field(s: &str) -> Option<String> {
    Regex::new(r#"addr="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(s).map(|c| c[1].to_string()))
}

pub(crate) fn parse_memory_contents(
    s: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    // Preferred MI form: memory=[{...,bytes="aabbcc"}]
    if let Some(caps) = Regex::new(r#"bytes="([0-9a-fA-F]+)""#)?.captures(s) {
        return hex_str_to_bytes(&caps[1]);
    }
    // Another form: contents="aa bb cc" or contents="aabbcc"
    if let Some(caps) = Regex::new(r#"contents="([^"]+)""#)?.captures(s) {
        let hex = &caps[1];
        if hex.contains(' ') {
            return Ok(split_hex_bytes(hex));
        } else {
            return hex_str_to_bytes(hex);
        }
    }
    // Common MI form: contents=["0xaa","0xbb",...]
    if let Some(caps) = Regex::new(r#"contents=\[([^\]]+)\]"#)?.captures(s) {
        return parse_hex_list(&caps[1]);
    }
    // Fallback for data=[...] form (legacy).
    if let Some(caps) = Regex::new(r#"data=\[([^\]]+)\]"#)?.captures(s) {
        return parse_hex_list(&caps[1]);
    }
    Err("no memory contents found".into())
}

fn parse_hex_list(list: &str) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let mut bytes = Vec::new();
    for part in list.split(',') {
        if let Some(b) = parse_hex_byte(part) {
            bytes.push(b);
        }
    }
    Ok(bytes)
}

pub(crate) fn split_hex_bytes(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for part in s.split_whitespace() {
        if let Some(b) = parse_hex_byte(part) {
            out.push(b);
        }
    }
    out
}

pub(crate) fn parse_locals(s: &str) -> Vec<LocalVar> {
    // MI locals are nested records; parse each {...} block and extract name/type/value separately
    // to avoid order sensitivity.
    let mut locals = Vec::new();
    let block_re = Regex::new(r"\{[^}]*\}").ok();
    let name_re = Regex::new(r#"name="([^"]+)""#).ok();
    let type_re = Regex::new(r#"type="((?:\\.|[^"])*)""#).ok();
    let value_re = Regex::new(r#"value="((?:\\.|[^"])*)""#).ok();

    if let (Some(block_re), Some(name_re)) = (block_re, name_re) {
        for block in block_re.find_iter(s) {
            let text = block.as_str();
            if let Some(name_caps) = name_re.captures(text) {
                let name = name_caps.get(1).map(|m| m.as_str().to_string());
                if let Some(name) = name {
                    let ty = type_re
                        .as_ref()
                        .and_then(|re| re.captures(text).map(|c| unescape_value(&c[1])));
                    let value = value_re
                        .as_ref()
                        .and_then(|re| re.captures(text).map(|c| unescape_value(&c[1])));
                    locals.push(LocalVar { name, ty, value });
                }
            }
        }
    }

    if locals.is_empty() {
        if let Ok(name_re) = Regex::new(r#"name="([^\"]+)""#) {
            for cap in name_re.captures_iter(s) {
                if let Some(name) = cap.get(1).map(|m| m.as_str().to_string()) {
                    let value = parse_value_field(s);
                    locals.push(LocalVar {
                        name,
                        ty: None,
                        value,
                    });
                }
            }
        }
    }
    locals
}

pub(crate) fn parse_usize(s: &str) -> std::result::Result<usize, String> {
    let trimmed = s.trim();
    if let Some(hex) = trimmed.strip_prefix("0x") {
        usize::from_str_radix(hex, 16).map_err(|e| format!("parse hex usize '{}': {}", trimmed, e))
    } else {
        trimmed
            .parse::<usize>()
            .map_err(|e| format!("parse usize '{}': {}", trimmed, e))
    }
}

pub(crate) fn bytes_to_u64(bytes: &[u8], endian: Endian) -> u64 {
    // Interpret up to 8 bytes from gdb in the current endianness, padding as needed.
    let mut buf = [0u8; 8];
    let len = bytes.len().min(8);
    if matches!(endian, Endian::Big) {
        buf[8 - len..].copy_from_slice(&bytes[..len]);
        u64::from_be_bytes(buf)
    } else {
        buf[..len].copy_from_slice(&bytes[..len]);
        u64::from_le_bytes(buf)
    }
}

pub(crate) fn parse_hex_byte(raw: &str) -> Option<u8> {
    let trimmed = raw.trim().trim_matches('"');
    if trimmed.is_empty() {
        return None;
    }
    let num = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    u8::from_str_radix(num, 16).ok()
}

pub(crate) fn hex_str_to_bytes(
    s: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    if s.len() % 2 != 0 {
        return Err("odd-length hex string in memory contents".into());
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let byte = &s[i..i + 2];
        let b = u8::from_str_radix(byte, 16)
            .map_err(|_| format!("invalid hex byte '{}' in memory contents", byte))?;
        out.push(b);
        i += 2;
    }
    Ok(out)
}

pub(crate) fn unescape_value(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.peek() {
                match *next {
                    '\\' => {
                        out.push('\\');
                        chars.next();
                        continue;
                    }
                    '"' => {
                        out.push('"');
                        chars.next();
                        continue;
                    }
                    'n' => {
                        out.push('\n');
                        chars.next();
                        continue;
                    }
                    't' => {
                        out.push('\t');
                        chars.next();
                        continue;
                    }
                    '0' => {
                        // Preserve explicit \0 / \000 sequences verbatim so downstream
                        // pretty-printers can decide how to show them.
                        out.push('\\');
                        out.push('0');
                        while let Some('0') = chars.peek() {
                            out.push('0');
                            chars.next();
                        }
                        continue;
                    }
                    _ => {}
                }
            }
        }
        out.push(c);
    }
    out
}

pub(crate) fn mi_escape(expr: &str) -> String {
    // Wrap an expression in MI-friendly quotes, escaping characters gdb/MI would treat specially.
    let mut out = String::with_capacity(expr.len() + 2);
    out.push('"');
    for ch in expr.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

pub(crate) fn parse_stopped(line: &str) -> StoppedLocation {
    let reason = Regex::new(r#"reason="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(line).map(|c| c[1].to_string()));
    let func = Regex::new(r#"func="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(line).map(|c| c[1].to_string()));
    let file = Regex::new(r#"file="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(line).map(|c| c[1].to_string()));
    let fullname = Regex::new(r#"fullname="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(line).map(|c| c[1].to_string()));
    let line_no = Regex::new(r#"line="([0-9]+)""#)
        .ok()
        .and_then(|re| re.captures(line).and_then(|c| c[1].parse::<u32>().ok()));
    let arch = Regex::new(r#"arch="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(line).map(|c| c[1].to_string()));
    StoppedLocation {
        func,
        file,
        fullname,
        line: line_no,
        reason,
        arch,
    }
}

pub(crate) fn parse_breakpoint(res: &str) -> BreakpointInfo {
    let num = Regex::new(r#"number="([0-9]+)""#)
        .ok()
        .and_then(|re| re.captures(res).and_then(|c| c[1].parse::<u32>().ok()))
        .unwrap_or(0);
    let func = Regex::new(r#"func="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(res).map(|c| c[1].to_string()));
    let file = Regex::new(r#"file="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(res).map(|c| c[1].to_string()));
    let line = Regex::new(r#"line="([0-9]+)""#)
        .ok()
        .and_then(|re| re.captures(res).and_then(|c| c[1].parse::<u32>().ok()));
    BreakpointInfo {
        number: num,
        file,
        line,
        func,
    }
}

pub(crate) fn parse_var_name(s: &str) -> Option<String> {
    Regex::new(r#"name="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(s).map(|c| c[1].to_string()))
}

/// Parse a simple `key="value"` field from MI text.
fn parse_field(s: &str, key: &str) -> Option<String> {
    let pattern = format!(r#"{key}="((?:\\.|[^"])*)""#);
    Regex::new(&pattern)
        .ok()
        .and_then(|re| re.captures(s).map(|c| unescape_value(&c[1])))
}

/// Extract the block following `key=`, returning the substring from the first `open_char`
/// through its matching `close_char`. String literals are skipped while tracking depth.
fn extract_block_after_key<'a>(
    src: &'a str,
    key: &str,
    open_char: char,
    close_char: char,
) -> Option<&'a str> {
    let key_pos = src.find(key)?;
    let after_key = &src[key_pos + key.len()..];
    let chars: Vec<(usize, char)> = after_key.char_indices().collect();

    let mut i = 0usize;
    // Skip separators immediately after the key.
    while i < chars.len() {
        let (_, ch) = chars[i];
        if ch == '=' || ch.is_whitespace() || ch == ',' {
            i += 1;
        } else {
            break;
        }
    }

    let mut start_byte = None;
    let mut depth = 0usize;

    while i < chars.len() {
        let (idx, ch) = chars[i];
        if ch == '"' {
            // Skip over string literals so braces/brackets inside them are ignored.
            i += 1;
            while i < chars.len() {
                let (_, inner) = chars[i];
                if inner == '\\' {
                    i = (i + 2).min(chars.len());
                    continue;
                }
                if inner == '"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        if start_byte.is_none() {
            if ch == open_char {
                start_byte = Some(idx);
                depth = 1;
            }
            i += 1;
            continue;
        }

        match ch {
            c if c == open_char => depth += 1,
            c if c == close_char => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let start = start_byte?;
                    return after_key.get(start..=idx);
                }
            }
            _ => {}
        }
        i += 1;
    }

    None
}

/// Split a list containing brace-delimited objects into individual `{...}` slices,
/// skipping over string literals while tracking brace depth.
fn split_braced_objects(list_src: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut depth = 0usize;
    let mut start_byte: Option<usize> = None;

    let chars: Vec<(usize, char)> = list_src.char_indices().collect();
    let mut i = 0usize;

    while i < chars.len() {
        let (idx, ch) = chars[i];
        match ch {
            '"' => {
                i += 1;
                while i < chars.len() {
                    let (_, inner) = chars[i];
                    if inner == '\\' {
                        i = (i + 2).min(chars.len());
                        continue;
                    }
                    if inner == '"' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            '{' => {
                if depth == 0 {
                    start_byte = Some(idx);
                }
                depth += 1;
            }
            '}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(start) = start_byte {
                            if let Some(slice) = list_src.get(start..=idx) {
                                result.push(slice);
                            }
                        }
                        start_byte = None;
                    }
                }
            }
            _ => {}
        }

        i += 1;
    }

    result
}

fn parse_symbol_from_value(s: &str) -> Option<MiSymbolVariable> {
    let name = parse_field(s, "name")?;
    let type_name = parse_field(s, "type");
    let line = parse_field(s, "line").and_then(|l| l.parse::<u32>().ok());
    let description = parse_field(s, "description");
    Some(MiSymbolVariable {
        name,
        type_name,
        line,
        description,
    })
}

fn parse_group_list(raw: &str, target_basename: Option<&str>) -> Vec<MiSymbolFileGroup> {
    let mut groups = Vec::new();
    for block in split_braced_objects(raw) {
        if let Some(tb) = target_basename {
            if !block.contains(tb) {
                continue;
            }
        }
        let filename = parse_field(block, "filename");
        let fullname = parse_field(block, "fullname");

        // symbols=[{...}] or variables=[{...}]
        let symbols_text = extract_block_after_key(block, "symbols", '[', ']')
            .or_else(|| extract_block_after_key(block, "variables", '[', ']'));
        let mut symbols = Vec::new();
        if let Some(list) = symbols_text {
            for sym in split_braced_objects(list) {
                if let Some(parsed) = parse_symbol_from_value(sym) {
                    symbols.push(parsed);
                }
            }
        } else if let Some(sym) = parse_symbol_from_value(block) {
            // Fallback: current tuple itself is a symbol
            symbols.push(sym);
        }

        groups.push(MiSymbolFileGroup {
            filename,
            fullname,
            symbols,
        });

        if target_basename.is_some() {
            break;
        }
    }
    groups
}

pub(crate) fn parse_symbol_info_variables(
    raw: &str,
    target_basename: Option<&str>,
) -> MiSymbolInfoVariables {
    // Try symbols={...} first (common with --include-nondebug).
    if let Some(symbols_block) = extract_block_after_key(raw, "symbols", '{', '}') {
        let mut info = MiSymbolInfoVariables::default();
        if let Some(debug_block) = extract_block_after_key(symbols_block, "debug", '[', ']') {
            info.debug = parse_group_list(&debug_block, target_basename);
        }
        if let Some(nondebug_block) = extract_block_after_key(symbols_block, "nondebug", '[', ']') {
            info.nondebug = parse_group_list(&nondebug_block, target_basename);
        }

        if !info.debug.is_empty() || !info.nondebug.is_empty() {
            return info;
        }
    }

    // Fallback: top-level symbols=[{...}] without debug/nondebug buckets.
    if let Some(vars_block) = extract_block_after_key(raw, "symbols", '[', ']') {
        let symbols = parse_group_list(&vars_block, target_basename);
        if !symbols.is_empty() {
            return MiSymbolInfoVariables {
                debug: symbols,
                nondebug: Vec::new(),
            };
        }
    }

    // Fallback: top-level variables=[{...}]
    if let Some(vars_block) = extract_block_after_key(raw, "variables", '[', ']') {
        let symbols = parse_group_list(&vars_block, target_basename);
        if !symbols.is_empty() {
            return MiSymbolInfoVariables {
                debug: symbols,
                nondebug: Vec::new(),
            };
        }
    }

    MiSymbolInfoVariables::default()
}

pub(crate) fn parse_endian(val: &str) -> Endian {
    let lower = val.to_ascii_lowercase();
    if lower.contains("little") {
        Endian::Little
    } else if lower.contains("big") {
        Endian::Big
    } else {
        Endian::Unknown
    }
}

pub(crate) fn guess_endian_from_arch(arch: &str) -> Option<Endian> {
    let a = arch.to_ascii_lowercase();
    if a.contains("x86") || a.contains("amd64") || a.contains("i386") {
        return Some(Endian::Little);
    }
    if a.contains("aarch64") || a.contains("arm") {
        return Some(Endian::Little);
    }
    if a.contains("riscv") {
        return Some(Endian::Little);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unescape_value_handles_common_sequences() {
        assert_eq!(unescape_value("foo\\nbar"), "foo\nbar");
        assert_eq!(unescape_value("foo\\\"bar"), "foo\"bar");
        assert_eq!(unescape_value("foo\\\\bar"), "foo\\bar");
    }

    #[test]
    fn test_parse_value_field_decodes_escaped_content() {
        let val = r#"value="hello\\nworld""#;
        assert_eq!(parse_value_field(val).unwrap(), "hello\\nworld");
    }

    #[test]
    fn test_hex_parsing_variants() {
        let a = parse_memory_contents(r#"bytes="aabbcc""#).unwrap();
        assert_eq!(a, vec![0xaa, 0xbb, 0xcc]);

        let b = parse_memory_contents(r#"contents="aa bb cc""#).unwrap();
        assert_eq!(b, vec![0xaa, 0xbb, 0xcc]);

        let c = parse_memory_contents(r#"contents=["0xaa","0xbb","0xcc"]"#).unwrap();
        assert_eq!(c, vec![0xaa, 0xbb, 0xcc]);
    }

    #[test]
    fn test_bytes_to_u64_endian() {
        let little = bytes_to_u64(&[0x01, 0x02, 0x03, 0x04], Endian::Little);
        assert_eq!(little, 0x04030201);
        let big = bytes_to_u64(&[0x01, 0x02, 0x03, 0x04], Endian::Big);
        assert_eq!(big, 0x01020304);
    }

    #[test]
    fn test_parse_locals_extracts_fields() {
        let raw = r#"{name="x",type="int",value="1"},{name="s",type="char *",value="foo"}"#;
        let locals = parse_locals(raw);
        assert_eq!(locals.len(), 2);
        assert_eq!(locals[0].name, "x");
        assert_eq!(locals[0].ty.as_deref(), Some("int"));
        assert_eq!(locals[1].value.as_deref(), Some("foo"));
    }

    #[test]
    fn test_extract_block_after_key_handles_nested_lists() {
        let raw = r#"^done,symbols={debug=[{name="a"},{name="b"}],nondebug=[{name="c"}]}"#;
        let symbols_block = extract_block_after_key(raw, "symbols", '{', '}').unwrap();
        let debug_block = extract_block_after_key(symbols_block, "debug", '[', ']').unwrap();
        let objects = split_braced_objects(debug_block);
        assert_eq!(objects.len(), 2);
        assert!(objects[0].contains(r#"name="a""#));
        let nondebug_block = extract_block_after_key(symbols_block, "nondebug", '[', ']').unwrap();
        let nondebug_objects = split_braced_objects(nondebug_block);
        assert_eq!(nondebug_objects.len(), 1);
    }

    #[test]
    fn test_parse_symbol_info_variables_parses_nested_groups() {
        let raw = r#"^done,symbols={
    debug=[
      {filename="../dlfcn/dlerror.h",
       fullname="/usr/src/glibc/dlfcn/dlerror.h",
       symbols=[{name="__libc_dlerror_result",type="struct dl_action_result",line="83",description="{in braces}"}]},
      {filename="sample.c",
       fullname="/home/user/sample.c",
       symbols=[{name="g_counter",type="int",line="12"},{name="flag",description="flag value"}]}
    ],
    nondebug=[
      {filename="../stdlib/strtol_l.c",
       symbols=[{name="strtol_l_internal",type="int"}]}
    ]
}"#;

        let parsed = parse_symbol_info_variables(raw, None);
        assert_eq!(parsed.debug.len(), 2);
        assert_eq!(parsed.nondebug.len(), 1);
        assert_eq!(
            parsed.debug[0].filename.as_deref(),
            Some("../dlfcn/dlerror.h")
        );
        assert_eq!(
            parsed.debug[0].symbols[0].description.as_deref(),
            Some("{in braces}")
        );
        assert_eq!(parsed.debug[1].symbols.len(), 2);
        assert_eq!(parsed.debug[1].symbols[0].line, Some(12));
        assert_eq!(parsed.nondebug[0].symbols[0].name, "strtol_l_internal");

        let total: usize = parsed
            .debug
            .iter()
            .chain(parsed.nondebug.iter())
            .map(|g| g.symbols.len())
            .sum();
        assert_eq!(total, 4);
    }
}
