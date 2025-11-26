use crate::mi::{BreakpointInfo, LocalVar, MemoryDump, MiSession, Result, StoppedLocation};
use std::io::{self, Write};

const MEM_READ_LEN: usize = 32;

pub fn repl(session: &mut MiSession) -> Result<()> {
    println!("Commands: locals | mem <symbol> | break <loc> | next | step | continue | help | quit");
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
                let symbol = rest;
                if symbol.is_empty() {
                    println!("usage: mem <symbol>");
                    continue;
                }
                match session.evaluate_address(symbol) {
                    Ok(addr) => match session.read_memory(&addr, MEM_READ_LEN) {
                        Ok(dump) => print_memory(&dump),
                        Err(e) => eprintln!("memory read failed: {}", e),
                    },
                    Err(e) => {
                        println!(
                            "symbol '{}' not found in current frame (only simple symbol names like 'x', 'arr', 'node' are supported in this version). {}",
                            symbol,
                            e
                        );
                    }
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
    println!("  mem <symbol>      - hex dump 32 bytes at &<symbol> (simple symbol names only)");
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
    println!("address: {}", dump.address);
    if dump.bytes.is_empty() {
        println!("bytes(0): (no bytes read)");
        return;
    }
    println!("bytes({}):", dump.bytes.len());
    for (i, chunk) in dump.bytes.chunks(16).enumerate() {
        let offset = i * 16;
        let hex: Vec<String> = chunk.iter().map(|b| format!("{:02x}", b)).collect();
        println!("  0x{:02x}: {}", offset, hex.join(" "));
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
    match (&loc.file, &loc.line, &loc.func) {
        (Some(f), Some(l), Some(func)) => println!("stopped at {}:{} ({})", f, l, func),
        (Some(f), Some(l), None) => println!("stopped at {}:{}", f, l),
        _ => println!("stopped (location unknown)"),
    }
}
