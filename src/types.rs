use regex::Regex;

#[derive(Debug, Clone)]
pub enum TypeLayout {
    Scalar {
        type_name: String,
        size: usize,
    },
    Array {
        type_name: String,
        elem_type: String,
        elem_size: usize,
        len: usize,
        #[allow(dead_code)]
        size: usize,
    },
    Struct {
        name: String,
        size: usize,
        fields: Vec<FieldLayout>,
    },
}

#[derive(Debug, Clone)]
pub struct FieldLayout {
    pub name: String,
    pub type_name: String,
    pub offset: usize,
    pub size: usize,
}

/// Very small ptype parser for simple structs/arrays/scalars.
pub fn parse_ptype_output(text: &str, word_size: usize, fallback_size: usize) -> TypeLayout {
    // Try array form: "type = int [5]"
    if let Some(layout) = parse_array_line(text, word_size) {
        return layout;
    }
    // Try struct form.
    if let Some(layout) = parse_struct_block(text, word_size) {
        return layout;
    }
    // Fallback scalar: take the first word after "type ="
    let ty = text
        .lines()
        .find_map(|l| l.trim_start().strip_prefix("type ="))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    TypeLayout::Scalar {
        type_name: ty,
        size: fallback_size,
    }
}

fn parse_array_line(text: &str, word_size: usize) -> Option<TypeLayout> {
    // crude: look for "type = <elem> [N]"
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("type =") {
            let parts: Vec<_> = rest.trim().split_whitespace().collect();
            if parts.len() >= 2 {
                let ty = parts[0].to_string();
                if let Some(len_str) = parts[1]
                    .trim()
                    .strip_prefix('[')
                    .and_then(|s| s.strip_suffix(']'))
                {
                    if let Ok(len) = len_str.parse::<usize>() {
                        let elem_size = base_type_size(&ty, word_size);
                        let size = elem_size.saturating_mul(len);
                        return Some(TypeLayout::Array {
                            type_name: format!("{} [{}]", ty, len),
                            elem_type: ty,
                            elem_size,
                            len,
                            size,
                        });
                    }
                }
            }
        }
    }
    None
}

fn parse_struct_block(text: &str, word_size: usize) -> Option<TypeLayout> {
    // Very small parser: assumes flat "type name;" lines inside the struct and estimates size
    // by accumulating base sizes (no padding handling).
    let mut lines = text.lines();
    let header = lines.find(|l| l.contains("type = struct"))?;
    let name = Regex::new(r"type\s*=\s*struct\s+([A-Za-z0-9_]+)")
        .ok()
        .and_then(|re| re.captures(header).map(|c| c[1].to_string()))
        .unwrap_or_else(|| "struct".to_string());
    let mut fields = Vec::new();
    let mut offset = 0usize;
    for line in lines {
        let tline = line.trim();
        if tline.starts_with('}') {
            break;
        }
        if tline.is_empty() {
            continue;
        }
        let cleaned = tline.trim_end_matches(';').trim();
        let (type_part, name_part) = match cleaned.rsplit_once(' ') {
            Some(v) => v,
            None => continue,
        };
        let mut field_type = type_part.trim().to_string();
        let mut field_name = name_part.trim().to_string();
        // Move leading '*' from name into the type to normalize pointer syntax.
        while field_name.starts_with('*') {
            field_name.remove(0);
            field_type.push_str(" *");
        }
        let mut size = base_type_size(&field_type, word_size);

        // array field
        if let Some(idx) = field_name.find('[') {
            let base_name = field_name[..idx].to_string();
            let len_str = field_name[idx + 1..].trim_end_matches(']');
            if let Ok(len) = len_str.parse::<usize>() {
                size = size.saturating_mul(len);
                field_type = format!("{}[{}]", field_type, len);
                field_name = base_name;
            }
        }

        fields.push(FieldLayout {
            name: field_name,
            type_name: field_type,
            offset,
            size,
        });
        offset = offset.saturating_add(size);
    }
    if fields.is_empty() {
        None
    } else {
        Some(TypeLayout::Struct {
            name,
            size: offset,
            fields,
        })
    }
}

fn base_type_size(type_name: &str, word_size: usize) -> usize {
    // Crude size guesser for simple C types; pointer width falls back to detected word size.
    let t = type_name.trim();
    if t.ends_with('*') {
        return word_size.max(1);
    }
    match t {
        "char" | "unsigned char" | "signed char" => 1,
        "short" | "unsigned short" => 2,
        "int" | "unsigned int" => 4,
        "long" | "unsigned long" | "long int" | "unsigned long int" => word_size.max(4),
        "long long" | "unsigned long long" => 8,
        "float" => 4,
        "double" => 8,
        _ => word_size.max(1),
    }
}

/// Normalize type string for display (e.g., "int [5]" -> "int[5]").
pub fn normalize_type_name(s: &str) -> String {
    // Remove spaces before array brackets to make output more compact/readable.
    let trimmed = s.trim();
    let mut out = String::with_capacity(trimmed.len());
    let mut chars = trimmed.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ' ' {
            if let Some('[') = chars.peek() {
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// Find a pointer field inside a struct, preferring a field literally named "next".
pub fn find_pointer_field(layout: &TypeLayout) -> Option<&FieldLayout> {
    if let TypeLayout::Struct { fields, .. } = layout {
        let mut first_ptr = None;
        for f in fields {
            if is_pointer_type(&f.type_name) {
                if f.name == "next" {
                    return Some(f);
                }
                if first_ptr.is_none() {
                    first_ptr = Some(f);
                }
            }
        }
        return first_ptr;
    }
    None
}

/// Basic pointer type heuristic: contains '*' and is not an array declaration.
pub fn is_pointer_type(ty: &str) -> bool {
    let t = ty.trim();
    t.contains('*') && !t.contains('[') && !t.contains(']')
}

/// Strip trailing '*' characters and surrounding spaces from a pointer type name.
pub fn strip_pointer_suffix(ty: &str) -> String {
    let mut trimmed = ty.trim().to_string();
    while trimmed.ends_with('*') {
        trimmed.pop();
    }
    trimmed.trim().to_string()
}

/// Normalize pointer type spacing for display (e.g., "struct Node *" -> "struct Node*").
pub fn normalize_pointer_type(ty: &str) -> String {
    normalize_type_name(ty).replace(" *", "*")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_type_name_removes_array_spaces() {
        assert_eq!(normalize_type_name("int [5]"), "int[5]");
        assert_eq!(normalize_type_name("struct Node *"), "struct Node *");
    }

    #[test]
    fn base_type_size_matches_word_size_for_pointers() {
        assert_eq!(base_type_size("int *", 8), 8);
        assert_eq!(base_type_size("char", 4), 1);
    }

    #[test]
    fn parse_ptype_handles_array() {
        let text = "type = int [5]";
        let layout = parse_ptype_output(text, 8, 4);
        match layout {
            TypeLayout::Array {
                elem_size, len, ..
            } => {
                assert_eq!(elem_size, 4);
                assert_eq!(len, 5);
            }
            _ => panic!("expected array"),
        }
    }

    #[test]
    fn parse_ptype_handles_struct() {
        let text = r#"
type = struct Node {
    int id;
    int count;
    char name[16];
    struct Node *next;
}
"#;
        let layout = parse_ptype_output(text, 8, 4);
        match layout {
            TypeLayout::Struct { fields, size, .. } => {
                assert_eq!(fields.len(), 4);
                assert_eq!(size, 32);
            }
            _ => panic!("expected struct"),
        }
    }
}
