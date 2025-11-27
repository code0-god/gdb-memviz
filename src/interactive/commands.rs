use super::follow;
use super::printers::{
    print_breakpoint, print_locals, print_memory_body, print_memory_full, print_stopped,
    print_vm_locate, print_vm_regions, VmLocateInfo,
};
use crate::mi::{MiSession, Result};
use crate::types::{is_pointer_type, normalize_type_name, strip_pointer_suffix, TypeLayout};
use crate::vm;
use std::num::ParseIntError;

pub enum CommandOutcome {
    Continue,
    Quit,
}

pub fn execute_command(
    input: &str,
    cmd: &str,
    rest: &str,
    session: &mut MiSession,
) -> Result<CommandOutcome> {
    // Special-case vm parsing to catch invalid usages.
    if cmd == "vm" {
        let parts: Vec<_> = input.trim().split_whitespace().collect();
        if parts.len() == 1 {
            handle_vm(session);
            return Ok(CommandOutcome::Continue);
        }
        if parts.len() >= 2 && parts[1] == "locate" {
            if parts.len() >= 3 {
                let expr = parts[2..].join(" ");
                handle_vm_locate(&expr, session);
            } else {
                eprintln!(
                    "invalid vm usage: '{}'\n  usage: vm\n         vm locate <expr>",
                    input.trim()
                );
            }
            return Ok(CommandOutcome::Continue);
        }
        eprintln!(
            "invalid vm usage: '{}'\n  usage: vm\n         vm locate <expr>",
            input.trim()
        );
        return Ok(CommandOutcome::Continue);
    }

    match cmd {
        "quit" | "q" => return Ok(CommandOutcome::Quit),
        "help" => print_help(),
        "locals" => match session.list_locals() {
            Ok(locals) => print_locals(&locals),
            Err(e) => eprintln!("locals error: {}", e),
        },
        "mem" => handle_mem(rest, session),
        "view" => {
            if rest.is_empty() {
                println!("usage: view <symbol>");
            } else {
                let symbol = rest.split_whitespace().next().unwrap_or("");
                if let Err(e) = handle_view(symbol, session) {
                    eprintln!("{}", e);
                }
            }
        }
        "follow" => {
            if rest.is_empty() {
                println!("usage: follow <symbol> [depth]");
            } else if let Err(e) = follow::handle_follow(rest, session) {
                eprintln!("{}", e);
            }
        }
        "break" | "b" => {
            if rest.is_empty() {
                println!("usage: break <location>");
            } else {
                match session.break_insert(rest) {
                    Ok(info) => print_breakpoint(&info),
                    Err(e) => eprintln!("break error: {}", e),
                }
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
        _ => {
            println!("unknown command: '{}'", input);
        }
    }
    Ok(CommandOutcome::Continue)
}

fn handle_vm(session: &mut MiSession) {
    let pid = match session.inferior_pid() {
        Ok(pid) => pid,
        Err(e) => {
            eprintln!("vm: could not determine inferior pid: {}", e);
            return;
        }
    };
    match vm::read_proc_maps(pid) {
        Ok(regions) => print_vm_regions(&regions),
        Err(e) => eprintln!("vm: failed to read /proc/{}: {}", pid, e),
    }
}

fn handle_vm_locate(sym: &str, session: &mut MiSession) {
    let pid = match session.inferior_pid() {
        Ok(pid) => pid,
        Err(e) => {
            eprintln!("vm locate: could not determine inferior pid: {}", e);
            return;
        }
    };
    let regions = match vm::read_proc_maps(pid) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("vm locate: failed to read /proc/{}: {}", pid, e);
            return;
        }
    };

    match resolve_vm_locate(session, sym, &regions) {
        Ok(info) => print_vm_locate(&info),
        Err(e) => eprintln!("vm locate: could not resolve '{}': {}", sym, e),
    }
}

fn parse_address(s: &str) -> Result<u64> {
    // Try hex first
    if let Some(hex) = s.trim().strip_prefix("0x") {
        return u64::from_str_radix(hex, 16).map_err(|e| e.into());
    }
    // Decimal fallback
    s.trim()
        .parse::<u64>()
        .map_err(|e: ParseIntError| e.into())
}

fn resolve_vm_locate<'a>(
    session: &mut MiSession,
    expr: &str,
    regions: &'a [vm::VmRegion],
) -> Result<VmLocateInfo<'a>> {
    let (expr_type, expr_value) = session.eval_expr_type_and_value(expr)?;
    let is_ptr = is_pointer_type(&expr_type) && expr_value.trim().starts_with("0x");

    if is_ptr {
        let storage_addr = session.eval_address_of_expr(expr)?;
        let storage_region = regions.iter().find(|r| r.contains(storage_addr));

        let mut is_null = false;
        let mut value_addr = None;
        let mut value_region = None;
        let val_trim = expr_value.trim();
        if val_trim == "0x0" || val_trim == "0" {
            is_null = true;
        } else if val_trim.starts_with("0x") {
            if let Ok(addr) = parse_address(val_trim) {
                value_addr = Some(addr);
                value_region = regions.iter().find(|r| r.contains(addr));
            }
        }

        Ok(VmLocateInfo {
            expr: expr.to_string(),
            type_name: expr_type,
            storage_addr: Some(storage_addr),
            storage_region,
            value_addr,
            value_region,
            is_pointer: true,
            is_null,
        })
    } else {
        let obj_addr = session.eval_address_of_expr(expr)?;
        let obj_region = regions.iter().find(|r| r.contains(obj_addr));
        Ok(VmLocateInfo {
            expr: expr.to_string(),
            type_name: expr_type,
            storage_addr: None,
            storage_region: None,
            value_addr: Some(obj_addr),
            value_region: obj_region,
            is_pointer: false,
            is_null: false,
        })
    }
}

fn handle_mem(rest: &str, session: &mut MiSession) {
    if rest.is_empty() {
        println!("usage: mem <expr> [len]");
        return;
    }
    let mut rest_parts = rest.split_whitespace();
    let expr = rest_parts.next().unwrap_or("");
    let len_opt = rest_parts.next().map(|s| s.parse::<usize>());
    // Optional length override; otherwise sizeof(expr) is used inside memory_dump.
    let override_len = match len_opt {
        Some(Ok(v)) => Some(v),
        Some(Err(_)) => {
            println!("invalid length: {}", rest);
            return;
        }
        None => None,
    };
    match session.memory_dump(expr, override_len) {
        Ok(dump) => print_memory_full(&dump),
        Err(e) => eprintln!("mem error: {}", e),
    }
}

fn handle_view(symbol: &str, session: &mut MiSession) -> Result<()> {
    // Make sure endian is resolved before printing layout info.
    session.ensure_endian();
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
    let ptype_line = session
        .ptype_text(symbol)
        .ok()
        .and_then(|txt| extract_type_line(&txt));

    // Try to get struct/array layout; fall back to scalar with known size.
    let layout = session
        .fetch_layout(symbol, size)
        .unwrap_or(TypeLayout::Scalar {
            type_name: "unknown".to_string(),
            size,
        });

    let type_display = ptype_line
        .as_ref()
        .map(|t| normalize_type_name(t))
        .unwrap_or_else(|| normalize_type_name(&type_name(&layout)));

    println!("symbol: {} ({}) @ {}", symbol, type_display, addr);
    println!("size: {} bytes (word size = {})", size, session.word_size);
    let endian_str = match session.endian {
        crate::mi::Endian::Little => "little-endian",
        crate::mi::Endian::Big => "big-endian",
        crate::mi::Endian::Unknown => "endian-unknown",
    };
    let arch_str = session.arch.as_deref().unwrap_or("unknown");
    println!("layout: {} (arch={})", endian_str, arch_str);

    // If the symbol itself is a pointer, treat it as such and do not print the pointee's layout
    // to avoid misrepresenting the pointer as a struct/array.
    if let Some(tline) = &ptype_line {
        if is_pointer_type(tline) {
            let pointee = strip_pointer_suffix(tline);
            println!("pointee type: {}", normalize_type_name(&pointee));
            println!("\nraw:");
            let dump = session.memory_dump(symbol, Some(size))?;
            print_memory_body(&dump);
            return Ok(());
        }
    }

    print_layout(&layout);

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

fn extract_type_line(ptype_text: &str) -> Option<String> {
    let header = ptype_text
        .lines()
        .find_map(|l| l.trim_start().strip_prefix("type =").map(|s| s.trim().to_string()))?;

    // Drop trailing struct opener if present: "struct Node {" -> "struct Node".
    let mut base = if let Some((head, _)) = header.split_once('{') {
        head.trim().to_string()
    } else {
        header
    };

    // gdb prints pointer-to-struct as a trailing "*"/"**" after the closing brace: "} *".
    if let Ok(re) = regex::Regex::new(r"}\s*(\*+)\s*$") {
        if let Some(caps) = re.captures(ptype_text) {
            let stars = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            if !stars.is_empty() {
                base = format!("{} {}", base, stars);
            }
        }
    }
    Some(base.trim().to_string())
}

fn print_layout(layout: &TypeLayout) {
    match layout {
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
}

fn print_help() {
    println!("Commands:");
    println!("  locals                - list locals in current frame");
    println!("  mem <expr> [len]      - hex+ASCII dump sizeof(<expr>) bytes (capped) at &<expr>; len overrides size");
    println!("  view <symbol>         - show type-based layout for symbol (struct/array) plus raw dump");
    println!("  follow <sym> [d]      - follow pointer chain for symbol up to optional depth (default ~8)");
    println!("  vm                    - show process memory map from /proc/<pid>/maps");
    println!("  vm locate <symbol>    - show which VM region contains the given symbol");
    println!("  break <loc> | b       - set breakpoint (e.g. 'break main', 'b file.c:42')");
    println!("  next | n              - execute next line (step over)");
    println!("  step | s              - step into functions");
    println!("  continue | c          - continue execution until next breakpoint");
    println!("  help                  - show this message");
    println!("  quit | q              - exit");
}
