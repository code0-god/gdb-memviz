use crate::mi::{BreakpointInfo, Endian, LocalVar, MemoryDump, MiSession, Result, StoppedLocation};
use std::io::{self, Write};

pub fn repl(session: &mut MiSession) -> Result<()> {
    println!("Commands: locals | mem <expr> [len] | break <loc> | next | step | continue | help | quit");
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
            "quit" => break,
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
                    Ok(dump) => print_memory(&dump),
                    Err(e) => eprintln!("mem error: {}", e),
                }
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
    println!("  mem <expr> [len]  - hex dump sizeof(<expr>) bytes (capped) at &<expr>; len overrides size");
    println!("  break <loc> | b   - set breakpoint (e.g. 'break main', 'b file.c:42')");
    println!("  next | n          - execute next line (step over)");
    println!("  step | s          - step into functions");
    println!("  continue | c      - continue execution until next breakpoint");
    println!("  help              - show this message");
    println!("  quit              - exit");
}

fn print_locals(locals: &[LocalVar]) {
    if locals.is_empty() {
        println!("no locals");
        return;
    }
    for (i, var) in locals.iter().enumerate() {
        let value = var.value.as_deref().unwrap_or("<unavailable>");
        let prefix = match var.ty.as_deref() {
            Some(ty) => format!("{} {}", ty, var.name),
            None => var.name.clone(),
        };
        println!("{}: {} = {}", i, prefix, value);
    }
}

fn print_memory(dump: &MemoryDump) {
    let ty = dump.ty.as_deref().unwrap_or("unknown");
    println!("symbol: {} ({})", dump.expr, ty);
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
    println!("words:");
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
            "  +0x{:02x}: {} | ascii=\"{}\"",
            offset,
            hex.join(" "),
            ascii_repr(&ascii_bytes)
        );
    }
}

fn print_breakpoint(bp: &BreakpointInfo) {
    let loc = match (&bp.file, &bp.line, &bp.func) {
        (Some(f), Some(l), _) => format!("{}:{}", f, l),
        (_, _, Some(func)) => func.clone(),
        _ => "<unknown>".to_string(),
    };
    println!("breakpoint {} at {}", bp.number, loc);
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
