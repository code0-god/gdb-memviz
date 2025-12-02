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

    // Handle special [xxx] mappings first
    if path == "[heap]" {
        VmLabel::Heap
    } else if path == "[stack]" {
        VmLabel::Stack
    } else if path.starts_with('[') && path.ends_with(']') {
        // Other bracketed regions like [vdso], [vvar], etc.
        VmLabel::Other(path.to_string())
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

/// Aggregated span for a given conceptual label, used by the VM minimap.
/// This is intentionally coarser than individual VmRegion entries.
#[derive(Debug, Clone)]
pub struct LabelSpan {
    pub label: VmLabel,
    pub start: u64,
    pub end: u64,
}

/// Conceptual bands for the VM minimap, ordered high → low.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VmBandKind {
    /// Top-of-user-space stack segment.
    Stack,
    /// Unallocated region between Stack and Lib (if any).
    Unallocated1,
    /// Memory-mapped region: shared libraries, anon mappings, etc.
    Lib,
    /// Unallocated region between Lib and Heap (if any).
    Unallocated2,
    /// Heap segment.
    Heap,
    /// Data/BSS.
    Data,
    /// Text / code segment.
    Text,
}

/// A band with an optional address span.
/// If span is None, that conceptual band has no concrete range in this process.
#[derive(Debug, Clone)]
pub struct VmBand {
    pub kind: VmBandKind,
    pub span: Option<(u64, u64)>, // [start, end)
}

/// Wrapper around VM regions that provides helper methods for layout and minimap rendering
#[derive(Debug, Clone)]
pub struct VmLayout {
    pub regions: Vec<VmRegion>,
}

impl VmLayout {
    /// Create a VmLayout by reading /proc/<pid>/maps
    pub fn from_proc_maps(pid: u32) -> io::Result<Self> {
        let regions = read_proc_maps(pid)?;
        Ok(Self { regions })
    }

    /// Refresh this layout in-place from /proc/<pid>/maps
    pub fn refresh_from_pid(&mut self, pid: u32) -> io::Result<()> {
        let regions = read_proc_maps(pid)?;
        self.regions = regions;
        Ok(())
    }

    /// Get the overall address range (min_start, max_end) across all regions
    /// Returns None if there are no regions
    pub fn addr_range(&self) -> Option<(u64, u64)> {
        if self.regions.is_empty() {
            return None;
        }

        let min_start = self.regions.iter().map(|r| r.start).min()?;
        let max_end = self.regions.iter().map(|r| r.end).max()?;

        Some((min_start, max_end))
    }

    /// Find the region containing the given address
    /// Returns the first region whose [start, end) contains addr
    pub fn region_at(&self, addr: u64) -> Option<&VmRegion> {
        self.regions.iter().find(|r| r.contains(addr))
    }

    /// Group regions by VmLabel and compute a single [start,end) span for each label.
    /// - All VmLabel::Other(..) are merged into one `Other("")` label.
    /// - Returns spans sorted by center address (low to high).
    pub fn label_spans(&self) -> Vec<LabelSpan> {
        use std::collections::HashMap;

        // Canonicalize labels so that all Other(..) map to the same key.
        fn canonical_label(label: &VmLabel) -> VmLabel {
            match label {
                VmLabel::Other(_) => VmLabel::Other(String::new()),
                other => other.clone(),
            }
        }

        let mut map: HashMap<VmLabel, (u64, u64)> = HashMap::new();

        for region in &self.regions {
            let key = canonical_label(&region.label);
            let entry = map.entry(key).or_insert((region.start, region.end));
            entry.0 = entry.0.min(region.start);
            entry.1 = entry.1.max(region.end);
        }

        let mut spans: Vec<LabelSpan> = map
            .into_iter()
            .filter_map(|(label, (start, end))| {
                if start < end {
                    Some(LabelSpan { label, start, end })
                } else {
                    None
                }
            })
            .collect();

        // Sort by center address (low → high).
        spans.sort_by_key(|s| (s.start + s.end) / 2);

        spans
    }

    /// Compute conceptual VM bands (high → low) for the minimap.
    /// The order is fixed: Stack, Unallocated1, Lib, Unallocated2, Heap, Data, Text.
    /// Each band carries an optional [start,end) span in real addresses.
    pub fn bands(&self) -> Vec<VmBand> {
        let text = aggregate_span(&self.regions, |r| matches!(r.label, VmLabel::Text));
        let data = aggregate_span(&self.regions, |r| matches!(r.label, VmLabel::Data));
        let heap = aggregate_span(&self.regions, |r| matches!(r.label, VmLabel::Heap));
        let stack = aggregate_span(&self.regions, |r| matches!(r.label, VmLabel::Stack));

        // Lib band aggregates Lib + Anonymous + Other (mmap-like) regions.
        let lib = aggregate_span(&self.regions, |r| {
            matches!(
                r.label,
                VmLabel::Lib | VmLabel::Anonymous | VmLabel::Other(_)
            )
        });

        // Unallocated2: between Lib and Heap (low side)
        let unalloc2 = match (heap, lib) {
            (Some((heap_start, _heap_end)), Some((_lib_start, lib_end))) => {
                if heap_start > lib_end {
                    Some((lib_end, heap_start))
                } else {
                    None
                }
            }
            _ => None,
        };

        // Unallocated1: between Stack and Lib (high side)
        let unalloc1 = match (stack, lib) {
            (Some((stack_start, _stack_end)), Some((lib_start, _lib_end))) => {
                if stack_start > lib_start {
                    Some((lib_start, stack_start))
                } else {
                    None
                }
            }
            _ => None,
        };

        vec![
            VmBand {
                kind: VmBandKind::Stack,
                span: stack,
            },
            VmBand {
                kind: VmBandKind::Unallocated1,
                span: unalloc1,
            },
            VmBand {
                kind: VmBandKind::Lib,
                span: lib,
            },
            VmBand {
                kind: VmBandKind::Unallocated2,
                span: unalloc2,
            },
            VmBand {
                kind: VmBandKind::Heap,
                span: heap,
            },
            VmBand {
                kind: VmBandKind::Data,
                span: data,
            },
            VmBand {
                kind: VmBandKind::Text,
                span: text,
            },
        ]
    }
}

/// Helper to aggregate regions matching a predicate into a single span.
fn aggregate_span<F>(regions: &[VmRegion], pred: F) -> Option<(u64, u64)>
where
    F: Fn(&VmRegion) -> bool,
{
    let mut start: Option<u64> = None;
    let mut end: Option<u64> = None;

    for r in regions.iter().filter(|r| pred(r)) {
        start = Some(start.map_or(r.start, |s| s.min(r.start)));
        end = Some(end.map_or(r.end, |e| e.max(r.end)));
    }

    match (start, end) {
        (Some(s), Some(e)) if s < e => Some((s, e)),
        _ => None,
    }
}

impl Default for VmLayout {
    fn default() -> Self {
        Self {
            regions: Vec::new(),
        }
    }
}

/// VM hexdump 패널 서브 포커스
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmHexPaneFocus {
    Address,
    Hex,
}

/// VM hexdump 뷰 상태 (VM Layout 패널의 Address/Hex/ASCII가 공유하는 상태)
#[derive(Debug, Clone)]
pub struct VmHexView {
    /// 한 줄에 표시할 바이트 수 (일단 16으로 사용)
    pub bytes_per_line: u16,
    /// 현재 화면에서 가장 위(첫 줄)에 표시되는 주소 (라인 시작 주소)
    pub top_addr: u64,
    /// 현재 선택된 바이트의 주소 (커서)
    pub cursor_addr: u64,
    /// 현재 화면에 몇 줄을 그리고 있는지 (렌더 단계에서 채움)
    pub lines_per_page: u16,
    /// top_addr 기준으로 읽어 온 원시 메모리 데이터
    pub buf: Vec<u8>,
    /// buf 안에서 실제 유효한 바이트 수 (read 실패 등으로 buf.len() 보다 작을 수 있음)
    pub valid_len: usize,
    /// 마지막으로 read_memory_bytes 로 로드한 페이지의 시작 주소
    pub last_loaded_top_addr: u64,
    /// 마지막으로 로드할 때 사용한 lines_per_page 값
    pub last_loaded_lines_per_page: u16,
}

impl VmHexView {
    pub fn new() -> Self {
        Self {
            bytes_per_line: 16,
            top_addr: 0,
            cursor_addr: 0,
            lines_per_page: 0,
            buf: Vec::new(),
            valid_len: 0,
            last_loaded_top_addr: u64::MAX, // 어떤 값과도 다르게 하기 위함
            last_loaded_lines_per_page: 0,
        }
    }

    pub fn page_size(&self) -> usize {
        (self.bytes_per_line as usize) * (self.lines_per_page as usize)
    }

    pub fn addr_at(&self, row: u16, col: u16) -> u64 {
        self.top_addr + (row as u64) * (self.bytes_per_line as u64) + (col as u64)
    }

    pub fn index_of(&self, addr: u64) -> Option<usize> {
        if addr < self.top_addr {
            return None;
        }
        let offset = addr - self.top_addr;
        if offset >= self.valid_len as u64 {
            None
        } else {
            Some(offset as usize)
        }
    }

    /// 현재 cursor_addr 가 페이지 안에 있으면 (row, col)를 반환.
    /// row/col 은 0 기반이며, row < lines_per_page, col < bytes_per_line 일 때만 Some.
    pub fn cursor_row_col(&self) -> Option<(u16, u16)> {
        if self.bytes_per_line == 0 || self.lines_per_page == 0 {
            return None;
        }
        let bpl = self.bytes_per_line as u64;

        if self.cursor_addr < self.top_addr {
            return None;
        }
        let diff = self.cursor_addr - self.top_addr;
        let row = (diff / bpl) as u16;
        let col = (diff % bpl) as u16;

        if row >= self.lines_per_page {
            return None;
        }
        Some((row, col))
    }

    /// 현재 페이지 기준 (row, col) 에 해당하는 실제 주소.
    /// (row, col)은 0 기반이라고 가정.
    pub fn addr_from_row_col(&self, row: u16, col: u16) -> u64 {
        let bpl = self.bytes_per_line as u64;
        self.top_addr + (row as u64) * bpl + (col as u64)
    }
}
