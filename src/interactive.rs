use crate::mi::{BreakpointInfo, Endian, LocalVar, MemoryDump, MiSession, Result, StoppedLocation};
use crate::types::{find_pointer_field, is_pointer_type, normalize_type_name, TypeLayout};
use regex::Regex;
use std::io::{self, Write};

pub fn repl(session: &mut MiSession) -> Result<()> {
    // Tiny read-eval-print loop: parse first token as command, rest as args, keep running
    // until EOF or quit.
    println!("Commands: locals | mem <expr> [len] | view <symbol> | follow <symbol> [depth] | break <loc> | next | step | continue | help | quit");
    let stdin = io::stdin();
    let mut line = String::new();
    loop {
        print!("memviz> ");
        io::stdout().flush()?;
        line.clear();
        if stdin.read_line(&mut line)? == 0 {
            println!();
            break;
        }
        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        let mut parts = input.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or("").trim();
        let rest = parts.next().unwrap_or("").trim();
        match cmd {
            "quit" | "q" => break,
            "help" => print_help(),
            "locals" => match session.list_locals() {
                Ok(locals) => print_locals(&locals),
                Err(e) => eprintln!("locals error: {}", e),
            },
            "mem" => {
                if rest.is_empty() {
                    println!("usage: mem <expr> [len]");
                    continue;
                }
                let mut rest_parts = rest.split_whitespace();
                let expr = rest_parts.next().unwrap_or("");
                let len_opt = rest_parts.next().map(|s| s.parse::<usize>());
                // Optional length override; otherwise sizeof(expr) is used inside memory_dump.
                let override_len = match len_opt {
                    Some(Ok(v)) => Some(v),
                    Some(Err(_)) => {
                        println!("invalid length: {}", rest);
                        continue;
                    }
                    None => None,
                };
                match session.memory_dump(expr, override_len) {
                    Ok(dump) => print_memory_full(&dump),
                    Err(e) => eprintln!("mem error: {}", e),
                }
            }
            "view" => {
                if rest.is_empty() {
                    println!("usage: view <symbol>");
                    continue;
                }
                let symbol = rest.split_whitespace().next().unwrap_or("");
                handle_view(symbol, session)?;
            }
            "follow" => {
                if rest.is_empty() {
                    println!("usage: follow <symbol> [depth]");
                    continue;
                }
                handle_follow(rest, session)?;
            }
            "break" | "b" => {
                if rest.is_empty() {
                    println!("usage: break <location>");
                    continue;
                }
                match session.break_insert(rest) {
                    Ok(info) => print_breakpoint(&info),
                    Err(e) => eprintln!("break error: {}", e),
                }
            }
            "next" | "n" => match session.exec_next() {
                Ok(loc) => print_stopped(&loc),
                Err(e) => eprintln!("next error: {}", e),
            },
            "step" | "s" => match session.exec_step() {
                Ok(loc) => print_stopped(&loc),
                Err(e) => eprintln!("step error: {}", e),
            },
            "continue" | "c" => match session.exec_continue() {
                Ok(loc) => print_stopped(&loc),
                Err(e) => eprintln!("continue error: {}", e),
            },
            _ => println!("unknown command: '{}'", input),
        }
    }
    Ok(())
}

fn print_help() {
    println!("Commands:");
    println!("  locals            - list locals in current frame");
    println!("  mem <expr> [len]  - hex+ASCII dump sizeof(<expr>) bytes (capped) at &<expr>; len overrides size");
    println!("  view <symbol>     - show type-based layout for symbol (struct/array) plus raw dump");
    println!("  follow <sym> [d]  - follow pointer chain for symbol up to optional depth (default ~8)");
    println!("  break <loc> | b   - set breakpoint (e.g. 'break main', 'b file.c:42')");
    println!("  next | n          - execute next line (step over)");
    println!("  step | s          - step into functions");
    println!("  continue | c      - continue execution until next breakpoint");
    println!("  help              - show this message");
    println!("  quit | q          - exit");
}

fn handle_view(symbol: &str, session: &mut MiSession) -> Result<()> {
    let size = match session.evaluate_sizeof(symbol) {
        Ok(sz) => sz,
        Err(e) => {
            println!("view: sizeof('{}') failed: {}", symbol, e);
            return Ok(());
        }
    };
    let addr = match session.evaluate_expression(&format!("&{}", symbol)) {
        Ok(v) => v,
        Err(e) => {
            println!("view: address for '{}' not found: {}", symbol, e);
            return Ok(());
        }
    };
    // Try to get struct/array layout; fall back to scalar with known size.
    let layout = session
        .fetch_layout(symbol, size)
        .unwrap_or(TypeLayout::Scalar {
            type_name: "unknown".to_string(),
            size,
        });
    println!(
        "symbol: {} ({}) @ {}",
        symbol,
        normalize_type_name(&type_name(&layout)),
        addr
    );
    println!("size: {} bytes (word size = {})", size, session.word_size);
    let endian_str = match session.endian {
        Endian::Little => "little-endian",
        Endian::Big => "big-endian",
        Endian::Unknown => "endian-unknown",
    };
    let arch_str = session.arch.as_deref().unwrap_or("unknown");
    println!("layout: {} (arch={})", endian_str, arch_str);
    match &layout {
        TypeLayout::Struct {
            fields, name: _, ..
        } => {
            println!("\nfields:");
            println!("  offset    size  field");
            for f in fields {
                println!(
                    "  +0x{:04x} {:>6}  {:<12} ({})",
                    f.offset,
                    f.size,
                    f.name,
                    normalize_type_name(&f.type_name)
                );
            }
        }
        TypeLayout::Array {
            elem_type,
            elem_size,
            len,
            ..
        } => {
            println!("\nelements:");
            println!("  offset    index  type");
            for i in 0..*len {
                let off = i * *elem_size;
                println!(
                    "  +0x{:04x} {:>7}  {}",
                    off,
                    format!("[{}]", i),
                    normalize_type_name(elem_type)
                );
            }
        }
        TypeLayout::Scalar { type_name, size } => {
            println!("\nscalar:\n  type: {}\n  size: {} bytes", type_name, size);
        }
    }
    println!("\nraw:");
    let dump = session.memory_dump(symbol, Some(size))?;
    print_memory_body(&dump);
    Ok(())
}

fn handle_follow(args: &str, session: &mut MiSession) -> Result<()> {
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
    let pointee_type = strip_pointer_type(ty);
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

fn type_name(layout: &TypeLayout) -> String {
    match layout {
        TypeLayout::Scalar { type_name, .. } => type_name.clone(),
        TypeLayout::Array { type_name, .. } => type_name.clone(),
        TypeLayout::Struct { name, .. } => format!("struct {}", name),
    }
}

fn print_locals(locals: &[LocalVar]) {
    if locals.is_empty() {
        println!("no locals");
        return;
    }
    for (i, var) in locals.iter().enumerate() {
        let value = var
            .value
            .as_ref()
            .map(|v| prettify_value(v))
            .unwrap_or_else(|| "<unavailable>".to_string());
        let prefix = match var.ty.as_deref() {
            Some(ty) => format!("{} {}", normalize_type_name(ty), var.name),
            None => var.name.clone(),
        };
        println!("{}: {} = {}", i, prefix, value);
    }
}

fn print_memory_full(dump: &MemoryDump) {
    let ty = dump.ty.as_deref().unwrap_or("unknown");
    println!("symbol: {} ({})", dump.expr, normalize_type_name(ty));
    println!("address: {}", dump.address);
    let size = dump.bytes.len();
    let words = (size + dump.word_size - 1) / dump.word_size.max(1);
    println!(
        "size: {} bytes (requested: {}, {} words, word size = {})",
        size, dump.requested, words, dump.word_size
    );
    let endian_str = match dump.endian {
        Endian::Little => "little-endian",
        Endian::Big => "big-endian",
        Endian::Unknown => "endian-unknown",
    };
    let arch_str = dump.arch.as_deref().unwrap_or("unknown");
    println!("layout: {} (arch={})", endian_str, arch_str);
    if let Some(orig) = dump.truncated_from {
        if orig > size {
            println!("(truncated to {} bytes from {})", size, orig);
        }
    }
    if dump.bytes.is_empty() {
        println!("bytes(0): (no bytes read)");
        return;
    }
    println!();
    println!("raw:");
    print_memory_body(dump);
}

fn print_breakpoint(bp: &BreakpointInfo) {
    let loc = match (&bp.file, &bp.line, &bp.func) {
        (Some(f), Some(l), _) => format!("{}:{}", f, l),
        (_, _, Some(func)) => func.clone(),
        _ => "<unknown>".to_string(),
    };
    println!("breakpoint {} at {}", bp.number, loc);
}

fn print_memory_body(dump: &MemoryDump) {
    let w = dump.word_size.max(1);
    for (i, chunk) in dump.bytes.chunks(w).enumerate() {
        let offset = i * w;
        let mut hex: Vec<String> = Vec::new();
        let mut ascii_bytes: Vec<u8> = Vec::new();
        for j in 0..w {
            if let Some(b) = chunk.get(j) {
                hex.push(format!("{:02x}", b));
                ascii_bytes.push(*b);
            } else {
                hex.push("..".to_string());
                ascii_bytes.push(b'.');
            }
        }
        println!(
            "  +0x{:04x}: {} | ascii=\"{}\"",
            offset,
            hex.join(" "),
            ascii_repr(&ascii_bytes)
        );
    }
}

fn print_stopped(loc: &StoppedLocation) {
    let where_str = match (&loc.file, &loc.line, &loc.func) {
        (Some(f), Some(l), Some(func)) => format!("stopped at {}:{} ({})", f, l, func),
        (Some(f), Some(l), None) => format!("stopped at {}:{}", f, l),
        _ => "stopped (location unknown)".to_string(),
    };
    if let Some(reason) = &loc.reason {
        println!("{} | reason: {}", where_str, reason);
    } else {
        println!("{}", where_str);
    }
}

fn ascii_repr(bytes: &[u8]) -> String {
    // Printable ASCII range is shown verbatim; everything else becomes '.'.
    bytes
        .iter()
        .map(|b| {
            let c = *b as char;
            if (0x20..=0x7e).contains(b) {
                c
            } else {
                '.'
            }
        })
        .collect()
}

fn prettify_value(s: &str) -> String {
    // Collapse gdb-style "'\000' <repeats N times>" into "\0 (xN)" for readability.
    let patterns = [
        r"'\\0+' <repeats ([0-9]+) times>",
        r"'\0+' <repeats ([0-9]+) times>",
    ];
    for pat in patterns {
        if let Ok(re) = Regex::new(pat) {
            let replaced = re.replace_all(s, "\\0 (x$1)").to_string();
            if replaced != s {
                return replaced;
            }
        }
    }
    // Also collapse contiguous raw \0 or \000 sequences (as emitted in array prints).
    if let Ok(re) = Regex::new(r"(\\0{1,3}){2,}") {
        if let Ok(single) = Regex::new(r"\\0{1,3}") {
            let replaced = re
                .replace_all(s, |caps: &regex::Captures| {
                    let matched = caps.get(0).map(|m| m.as_str()).unwrap_or("");
                    let count = single.find_iter(matched).count().max(1);
                    format!("\\0 (x{})", count)
                })
                .to_string();
            if replaced != s {
                return replaced;
            }
        }
    }
    s.to_string()
}

fn parse_pointer_address(value: &str) -> Option<u64> {
    // Try hex form first; fall back to decimal if hex is absent.
    if let Ok(re) = Regex::new(r"0x[0-9a-fA-F]+") {
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

fn strip_pointer_type(ty: &str) -> String {
    let mut trimmed = ty.trim().to_string();
    while trimmed.ends_with('*') {
        trimmed.pop();
    }
    trimmed.trim().to_string()
}

fn normalize_pointer_type(ty: &str) -> String {
    // Collapse spaces for display: "struct Node *" -> "struct Node*"
    normalize_type_name(ty).replace(" *", "*")
}

fn format_addr(addr: u64) -> String {
    format!("0x{:x}", addr)
}
