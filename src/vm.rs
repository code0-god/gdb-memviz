use std::fs::File;
use std::io;
use std::io::{BufRead, BufReader};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VmLabel {
    Text,      // executable text/code
    Data,      // data/bss
    Heap,      // [heap]
    Stack,     // [stack]
    Lib,       // shared libraries
    Anonymous, // anonymous mapping
    Other(String),
}

#[derive(Debug, Clone)]
pub struct VmRegion {
    pub start: u64,
    pub end: u64,
    pub perms: String,
    pub pathname: String,
    pub label: VmLabel,
}

impl VmRegion {
    pub fn size(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    pub fn contains(&self, addr: u64) -> bool {
        self.start <= addr && addr < self.end
    }
}

pub fn read_proc_maps(pid: u32) -> io::Result<Vec<VmRegion>> {
    let path = format!("/proc/{}/maps", pid);
    let file = File::open(&path)?;
    let reader = BufReader::new(file);

    let mut regions = Vec::new();

    for line_res in reader.lines() {
        let line = line_res?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let mut parts = trimmed.split_whitespace();
        let range = match parts.next() {
            Some(r) => r,
            None => continue,
        };
        let perms = match parts.next() {
            Some(p) => p.to_string(),
            None => continue,
        };

        let _offset = parts.next();
        let _dev = parts.next();
        let _inode = parts.next();

        let pathname = parts.collect::<Vec<_>>().join(" ");

        let (start_str, end_str) = match range.split_once('-') {
            Some(v) => v,
            None => continue,
        };
        let start = u64::from_str_radix(start_str, 16).unwrap_or(0);
        let end = u64::from_str_radix(end_str, 16).unwrap_or(0);
        if start >= end {
            continue;
        }

        let label = classify_region_label(&perms, &pathname);

        regions.push(VmRegion {
            start,
            end,
            perms,
            pathname,
            label,
        });
    }

    Ok(regions)
}

fn classify_region_label(perms: &str, pathname: &str) -> VmLabel {
    let path = pathname.trim();

    if path == "[heap]" {
        VmLabel::Heap
    } else if path == "[stack]" {
        VmLabel::Stack
    } else if path.is_empty() {
        VmLabel::Anonymous
    } else if path.contains("lib") || path.contains(".so") {
        VmLabel::Lib
    } else if perms.starts_with("r-x") {
        VmLabel::Text
    } else if perms.starts_with("rw-") {
        VmLabel::Data
    } else {
        VmLabel::Other(path.to_string())
    }
}

pub fn classify_addr(regions: &[VmRegion], addr: u64) -> &'static str {
    for r in regions {
        if r.contains(addr) {
            return match r.label {
                VmLabel::Text => "[text]",
                VmLabel::Data => "[data]",
                VmLabel::Heap => "[heap]",
                VmLabel::Stack => "[stack]",
                VmLabel::Lib => "[lib]",
                VmLabel::Anonymous => "[anon]",
                VmLabel::Other(_) => "[other]",
            };
        }
    }
    "[unknown]"
}
