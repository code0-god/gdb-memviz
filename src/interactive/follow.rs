use super::printers::prettify_value;
use crate::mi::{MiSession, Result};
use crate::types::{
    find_pointer_field, is_pointer_type, normalize_pointer_type, strip_pointer_suffix, TypeLayout,
};

pub fn handle_follow(args: &str, session: &mut MiSession) -> Result<()> {
    // Minimal pointer-chain walker: validates the symbol, figures out pointee layout,
    // then repeatedly evaluates the struct value and reads the chosen link field.
    let mut parts = args.split_whitespace();
    let symbol = match parts.next() {
        Some(s) if !s.is_empty() => s,
        _ => {
            println!("usage: follow <symbol> [depth]");
            return Ok(());
        }
    };
    let depth = match parts.next() {
        Some(raw) => match raw.parse::<usize>() {
            Ok(v) if v > 0 => v,
            Ok(_) => {
                println!("follow: depth must be positive");
                return Ok(());
            }
            Err(_) => {
                println!("follow: invalid depth '{}'", raw);
                return Ok(());
            }
        },
        None => 8,
    };
    let locals = match session.list_locals() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("follow: failed to list locals: {}", e);
            return Ok(());
        }
    };
    let var = match locals.iter().find(|v| v.name == symbol) {
        Some(v) => v,
        None => {
            println!("follow: symbol '{}' not found in locals", symbol);
            return Ok(());
        }
    };
    let ty = match &var.ty {
        Some(t) => t.trim(),
        None => {
            println!("follow: type for '{}' unavailable", symbol);
            return Ok(());
        }
    };
    if !is_pointer_type(ty) {
        println!("follow: '{}' is not a pointer type (got '{}')", symbol, ty);
        return Ok(());
    }
    let pointee_type = strip_pointer_suffix(ty);
    if pointee_type.is_empty() {
        println!("follow: cannot obtain layout for pointee type '{}'", ty);
        return Ok(());
    }
    let ptr_display = normalize_pointer_type(ty);

    let mut value_text = var.value.clone();
    if value_text.is_none() {
        value_text = session.evaluate_expression(symbol).ok();
    }
    // Parse the pointer address from gdb's string representation. If it doesn't parse,
    // try re-evaluating to get a simpler form.
    let raw_value = match value_text {
        Some(v) => v,
        None => {
            println!("follow: value for '{}' unavailable", symbol);
            return Ok(());
        }
    };
    let mut addr_opt = parse_pointer_address(&raw_value);
    if addr_opt.is_none() {
        if let Ok(eval) = session.evaluate_expression(symbol) {
            addr_opt = parse_pointer_address(&eval);
        }
    }
    let mut addr = match addr_opt {
        Some(a) => a,
        None => {
            println!(
                "follow: could not parse pointer value for '{}' (value '{}')",
                symbol, raw_value
            );
            return Ok(());
        }
    };
    if addr == 0 {
        println!("follow: '{}' is NULL", symbol);
        return Ok(());
    }

    let layout = match session.fetch_layout_for_type(&pointee_type) {
        Some(l @ TypeLayout::Struct { .. }) => l,
        Some(_) => {
            println!(
                "follow: cannot obtain layout for pointee type '{}'",
                pointee_type
            );
            return Ok(());
        }
        None => {
            println!(
                "follow: cannot obtain layout for pointee type '{}'",
                pointee_type
            );
            return Ok(());
        }
    };
    let struct_name = match &layout {
        TypeLayout::Struct { name, .. } => name.clone(),
        _ => pointee_type.clone(),
    };
    // Pick link field: prefer "next", otherwise the first pointer field we see.
    let link_field = match find_pointer_field(&layout).cloned() {
        Some(f) => f,
        None => {
            println!(
                "follow: struct {} has no pointer field to follow (expected e.g. 'next')",
                struct_name
            );
            return Ok(());
        }
    };

    let mut expr_display = symbol.to_string();
    for i in 0..depth {
        println!(
            "[{}] {} ({}) = {}",
            i,
            expr_display,
            ptr_display,
            format_addr(addr)
        );
        if addr == 0 {
            println!("    -> NULL (stopped)");
            break;
        }
        match session.evaluate_expression(&format!("* ({} *) (0x{:x})", pointee_type, addr)) {
            Ok(val) => println!("    -> {} {}", pointee_type, prettify_value(&val)),
            Err(e) => println!("    -> <eval error: {}>", e),
        }
        // Read the link field directly from memory to avoid parsing the evaluated struct.
        let field_addr = match addr.checked_add(link_field.offset as u64) {
            Some(v) => v,
            None => {
                println!("    -> overflow computing address for {}", link_field.name);
                break;
            }
        };
        let next_addr = match session.read_pointer_at(field_addr, Some(link_field.size)) {
            Ok(v) => v,
            Err(e) => {
                println!(
                    "    -> failed to read {}.{}: {}",
                    struct_name, link_field.name, e
                );
                break;
            }
        };
        expr_display = format!("{}->{}", expr_display, link_field.name);
        addr = next_addr;
    }
    Ok(())
}

fn parse_pointer_address(value: &str) -> Option<u64> {
    // Try hex form first; fall back to decimal if hex is absent.
    if let Ok(re) = regex::Regex::new(r"0x[0-9a-fA-F]+") {
        if let Some(mat) = re.find(value) {
            let trimmed = mat.as_str().trim_start_matches("0x");
            if let Ok(v) = u64::from_str_radix(trimmed, 16) {
                return Some(v);
            }
        }
    }
    let trimmed = value.trim();
    if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(v) = trimmed.parse::<u64>() {
            return Some(v);
        }
    }
    None
}

fn format_addr(addr: u64) -> String {
    format!("0x{:x}", addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pointer_addresses() {
        assert_eq!(parse_pointer_address("0x10"), Some(0x10));
        assert_eq!(parse_pointer_address(" 1234 "), Some(1234));
        assert!(parse_pointer_address("foo").is_none());
    }

    #[test]
    fn strip_pointer_removes_trailing_stars() {
        assert_eq!(strip_pointer_suffix("int ***"), "int");
        assert_eq!(strip_pointer_suffix("struct Node *"), "struct Node");
    }

    #[test]
    fn normalize_pointer_flattens_spaces() {
        assert_eq!(normalize_pointer_type("struct Node *"), "struct Node*");
    }
}
