use crate::mi::{BreakpointInfo, Endian, GlobalVar, LocalVar, MemoryDump, StoppedLocation};
use crate::vm::{classify_addr, VmLabel, VmRegion};
use crate::types::normalize_type_name;
use regex::Regex;

pub fn print_locals(locals: &[LocalVar]) {
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

pub fn print_memory_full(dump: &MemoryDump) {
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

pub fn print_breakpoint(bp: &BreakpointInfo) {
    let loc = match (&bp.file, &bp.line, &bp.func) {
        (Some(f), Some(l), _) => format!("{}:{}", f, l),
        (_, _, Some(func)) => func.clone(),
        _ => "<unknown>".to_string(),
    };
    println!("breakpoint {} at {}", bp.number, loc);
}

pub fn print_memory_body(dump: &MemoryDump) {
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

pub fn print_stopped(loc: &StoppedLocation) {
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

pub fn prettify_value(s: &str) -> String {
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

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn format_region_desc(region: &VmRegion) -> String {
    if region.pathname == "[heap]" {
        "(heap)".to_string()
    } else if region.pathname == "[stack]" {
        "(stack)".to_string()
    } else {
        region.pathname.clone()
    }
}

pub fn print_vm_regions(regions: &[VmRegion]) {
    println!("regions:");
    for r in regions {
        let label = match &r.label {
            VmLabel::Text => "[text]",
            VmLabel::Data => "[data]",
            VmLabel::Heap => "[heap]",
            VmLabel::Stack => "[stack]",
            VmLabel::Lib => "[lib]",
            VmLabel::Anonymous => "[anon]",
            VmLabel::Other(_) => "[other]",
        };
        let size_str = format_size(r.size());
        let desc = format_region_desc(r);

        if desc.is_empty() {
            println!(
                "  {:<8} 0x{:016x}-0x{:016x} ({}) {}",
                label, r.start, r.end, size_str, r.perms,
            );
        } else {
            println!(
                "  {:<8} 0x{:016x}-0x{:016x} ({}) {} {}",
                label, r.start, r.end, size_str, r.perms, desc,
            );
        }
    }
}

pub struct VmLocateInfo<'a> {
    pub expr: String,
    pub type_name: String,
    pub storage_addr: Option<u64>,
    pub storage_region: Option<&'a VmRegion>,
    pub value_addr: Option<u64>,
    pub value_region: Option<&'a VmRegion>,
    pub is_pointer: bool,
    pub is_null: bool,
}

pub fn print_vm_locate(info: &VmLocateInfo<'_>) {
    println!("expr: {} ({})", info.expr, info.type_name);
    if info.is_pointer {
        println!("  storage:");
        if let Some(addr) = info.storage_addr {
            println!("    addr:   0x{:016x}", addr);
            if let Some(region) = info.storage_region {
                let label = match &region.label {
                    VmLabel::Text => "[text]",
                    VmLabel::Data => "[data]",
                    VmLabel::Heap => "[heap]",
                    VmLabel::Stack => "[stack]",
                    VmLabel::Lib => "[lib]",
                    VmLabel::Anonymous => "[anon]",
                    VmLabel::Other(_) => "[other]",
                };
                let desc = format_region_desc(region);
                if desc.is_empty() {
                    println!(
                        "    region: {} 0x{:016x}-0x{:016x} {}",
                        label, region.start, region.end, region.perms
                    );
                } else {
                    println!(
                        "    region: {} 0x{:016x}-0x{:016x} {} {}",
                        label, region.start, region.end, region.perms, desc
                    );
                }
                let offset = addr.saturating_sub(region.start);
                println!("    offset: +0x{:x} from region base", offset);
            }
        }
        println!("  value:");
        if info.is_null {
            println!("    ptr:    0x0 (NULL)");
        } else if let Some(vaddr) = info.value_addr {
            println!("    ptr:    0x{:016x}", vaddr);
            if let Some(region) = info.value_region {
                let label = match &region.label {
                    VmLabel::Text => "[text]",
                    VmLabel::Data => "[data]",
                    VmLabel::Heap => "[heap]",
                    VmLabel::Stack => "[stack]",
                    VmLabel::Lib => "[lib]",
                    VmLabel::Anonymous => "[anon]",
                    VmLabel::Other(_) => "[other]",
                };
                let desc = format_region_desc(region);
                if desc.is_empty() {
                    println!(
                        "    region: {} 0x{:016x}-0x{:016x} {}",
                        label, region.start, region.end, region.perms
                    );
                } else {
                    println!(
                        "    region: {} 0x{:016x}-0x{:016x} {} {}",
                        label, region.start, region.end, region.perms, desc
                    );
                }
                let offset = vaddr.saturating_sub(region.start);
                println!("    offset: +0x{:x} from region base", offset);
            } else {
                println!("    region: <unknown>");
            }
        } else {
            println!("    ptr:    <unavailable>");
        }
    } else {
        println!("  object:");
        if let Some(vaddr) = info.value_addr {
            println!("    addr:   0x{:016x}", vaddr);
            if let Some(region) = info.value_region {
                let label = match &region.label {
                    VmLabel::Text => "[text]",
                    VmLabel::Data => "[data]",
                    VmLabel::Heap => "[heap]",
                    VmLabel::Stack => "[stack]",
                    VmLabel::Lib => "[lib]",
                    VmLabel::Anonymous => "[anon]",
                    VmLabel::Other(_) => "[other]",
                };
                let desc = format_region_desc(region);
                if desc.is_empty() {
                    println!(
                        "    region: {} 0x{:016x}-0x{:016x} {}",
                        label, region.start, region.end, region.perms
                    );
                } else {
                    println!(
                        "    region: {} 0x{:016x}-0x{:016x} {} {}",
                        label, region.start, region.end, region.perms, desc
                    );
                }
                let offset = vaddr.saturating_sub(region.start);
                println!("    offset: +0x{:x} from region base", offset);
            } else {
                println!("    region: <unknown>");
            }
        } else {
            println!("    addr:   <unavailable>");
        }
    }
}

#[allow(dead_code)]
fn label_for_global(regions: Option<&[VmRegion]>, addr: u64) -> &'static str {
    if let Some(rs) = regions {
        classify_addr(rs, addr)
    } else {
        "[unknown]"
    }
}

pub fn print_globals(globals: &[GlobalVar], _vm_regions: Option<&[VmRegion]>) {
    if globals.is_empty() {
        return;
    }
    for (idx, g) in globals.iter().enumerate() {
        let value = prettify_value(&g.value);
        println!("{}: {} {} = {}", idx, g.type_name, g.name, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mi::Endian;

    #[test]
    fn prettify_value_collapses_repeats() {
        assert_eq!(prettify_value("'\\000' <repeats 3 times>"), "\\0 (x3)");
        assert_eq!(prettify_value("plain"), "plain");
    }

    #[test]
    fn ascii_repr_replaces_non_printable() {
        assert_eq!(ascii_repr(&[0x41, 0x0, 0x7f]), "A..");
    }

    #[test]
    fn print_memory_body_formats_word_sized_chunks() {
        let dump = MemoryDump {
            expr: "x".into(),
            ty: Some("int".into()),
            address: "0x0".into(),
            bytes: vec![0x01, 0x02, 0x20, 0x41],
            word_size: 2,
            requested: 4,
            endian: Endian::Little,
            arch: None,
            truncated_from: None,
        };
        // Smoke-test: ensure it doesn't panic and lines are sensible.
        print_memory_body(&dump);
    }
}
