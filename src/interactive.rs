use crate::mi::{BreakpointInfo, Endian, LocalVar, MemoryDump, MiSession, Result, StoppedLocation};
use crate::types::{TypeLayout, normalize_type_name};
use std::io::{self, Write};

pub fn repl(session: &mut MiSession) -> Result<()> {
    println!("Commands: locals | mem <expr> [len] | view <symbol> | break <loc> | next | step | continue | help | quit");
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
    let layout = session.fetch_layout(symbol, size).unwrap_or(TypeLayout::Scalar {
        type_name: "unknown".to_string(),
        size,
    });
    println!("symbol: {} ({}) @ {}", symbol, normalize_type_name(&type_name(&layout)), addr);
    println!("size: {} bytes (word size = {})", size, session.word_size);
    let endian_str = match session.endian {
        Endian::Little => "little-endian",
        Endian::Big => "big-endian",
        Endian::Unknown => "endian-unknown",
    };
    let arch_str = session.arch.as_deref().unwrap_or("unknown");
    println!("layout: {} (arch={})", endian_str, arch_str);
    match &layout {
        TypeLayout::Struct { fields, name: _, .. } => {
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
        let value = var.value.as_deref().unwrap_or("<unavailable>");
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
