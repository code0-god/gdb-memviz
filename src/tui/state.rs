use crate::interactive::printers::prettify_value;
use crate::mi::{GlobalVar, LocalVar, MiSession, Result, StoppedLocation};
use crate::symbols::{GlobalVarWithValue, SymbolIndex, SymbolIndexMode};
use crate::tui::theme::Theme;
use crate::types::{normalize_pointer_type, normalize_type_name};
use crate::vm::{VmHexPaneFocus, VmHexView, VmLayout};
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

const SOURCE_PLACEHOLDER: &str = r#"examples/sample.c (placeholder)

int main(int argc, char **argv) {
    int x = 42;
    int y = argc + 7;
    // TODO: real source view (later)
}
"#;

const SYMBOLS_PLACEHOLDER: &str = r#"locals (placeholder):
  0: int x = 42
  1: int y = 8
  2: int[5] arr = {1, 2, 3, 4, 5}

globals (placeholder):
  g_counter: int = 7
  g_message: const char* = "hello"
"#;

const VM_LAYOUT_PLACEHOLDER: &str = r#"[VM Layout placeholder]

addr (high)
0x0000fffffffde000  [stack]  (grows down)
  #####################

0x0000aaaaaaab3000  [heap]   (grows up)
  ###..###############

0x0000aaaaaaab2000  [data]
  ###X###############

0x0000aaaaaaaa0000  [text]
  ###########

addr (low)
"#;

const DETAIL_PLACEHOLDER: &str = r#"Detail (placeholder):

  struct Node {
      int id;
      int count;
      char name[16];
      struct Node *next;
  };
"#;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PaneId {
    Source,
    Symbols,
    VmCanvas,
    Detail,
}

// Unified focus with PaneId
pub type Focus = PaneId;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SplitDir {
    Vertical,   // left | right
    Horizontal, // top  | bottom
}

#[derive(Clone, Debug)]
pub enum PaneNode {
    Leaf(PaneId),
    Split {
        dir: SplitDir,
        ratio: u8, // 0..=100, first child share in percent
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

#[derive(Clone, Debug)]
pub struct LayoutState {
    pub root: PaneNode,
}

impl Default for LayoutState {
    fn default() -> Self {
        Self {
            root: default_layout_tree(),
        }
    }
}

fn default_layout_tree() -> PaneNode {
    use PaneId::*;
    use SplitDir::*;

    // top row: Source | VmCanvas
    let top = PaneNode::Split {
        dir: Vertical,
        ratio: 50, // 50/50 for now
        first: Box::new(PaneNode::Leaf(Source)),
        second: Box::new(PaneNode::Leaf(VmCanvas)),
    };

    // bottom row: Symbols | Detail
    let bottom = PaneNode::Split {
        dir: Vertical,
        ratio: 50,
        first: Box::new(PaneNode::Leaf(Symbols)),
        second: Box::new(PaneNode::Leaf(Detail)),
    };

    // whole screen (without status bar): top (60%) over bottom (40%)
    PaneNode::Split {
        dir: Horizontal,
        ratio: 60,
        first: Box::new(top),
        second: Box::new(bottom),
    }
}

#[derive(Clone, Debug)]
pub struct SourceViewState {
    pub filename: Option<PathBuf>,
    pub lines: Vec<String>,
    pub current_line: Option<u32>,
    pub scroll_top: u32,
}

impl SourceViewState {
    pub fn new() -> Self {
        Self {
            filename: None,
            lines: Vec::new(),
            current_line: None,
            scroll_top: 0,
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub enum SymbolSection {
    #[default]
    Locals,
    Globals,
}

#[derive(Debug, Clone)]
pub struct SymbolEntry {
    pub name: String,
    pub type_name: String,
    pub value_preview: String, // Full display line: "type name = value"
}

#[derive(Debug, Default)]
pub struct SymbolsViewState {
    pub locals: Vec<SymbolEntry>,
    pub globals: Vec<SymbolEntry>,
    pub selected_section: SymbolSection,
    pub selected_index: usize,
}

#[derive(Debug, Clone)]
pub struct VmJumpState {
    pub active: bool,          // 점프 모드 팝업이 열려 있는지
    pub input: String,         // 사용자가 입력한 주소 문자열
    pub error: Option<String>, // 파싱/범위 오류 메시지 (없으면 None)
}

impl VmJumpState {
    pub fn new() -> Self {
        Self {
            active: false,
            input: String::new(),
            error: None,
        }
    }

    pub fn clear(&mut self) {
        self.input.clear();
        self.error = None;
    }
}

#[derive(Clone, Debug)]
pub struct VmView {
    pub lines: Vec<String>,
    pub scroll_y: u16,
    pub layout: VmLayout,
    pub cursor_addr: Option<u64>,
    pub hex: VmHexView,
    pub sub_focus: VmHexPaneFocus,
    pub jump: VmJumpState,     // 점프 상태
}

#[derive(Clone, Debug)]
pub struct DetailView {
    pub lines: Vec<String>,
    pub scroll_y: u16,
}

#[derive(Debug)]
pub struct AppState {
    pub theme: Theme,
    pub focus: Focus,
    pub layout: LayoutState,

    pub source: SourceViewState,
    pub symbols: SymbolsViewState,
    pub vm: VmView,
    pub detail: DetailView,
    pub debugger: MiSession,
    pub binary_path: PathBuf,
    pub symbol_index: Option<SymbolIndex>,
    pub symbol_index_mode: SymbolIndexMode,
    warned_stale_binary: bool,
    pub verbose: bool,

    // New fields for popup management
    pub show_symbols_popup: bool,
    pub last_main_focus: Focus,

    /// 좌측(Source) 영역 비율 (0~100), 기본 60%
    pub main_split: u16,

    /// Symbols 팝업 너비 (칼럼 수), 기본 40칼럼
    pub symbols_popup_width: u16,
}

impl AppState {
    pub fn new(
        debugger: MiSession,
        binary_path: PathBuf,
        symbol_index: Option<SymbolIndex>,
        symbol_index_mode: SymbolIndexMode,
        verbose: bool,
    ) -> Self {
        Self {
            theme: Theme::default(),
            focus: Focus::Source,
            layout: LayoutState::default(),
            source: SourceViewState::new(),
            symbols: SymbolsViewState::default(),
            vm: VmView {
                lines: split_lines(VM_LAYOUT_PLACEHOLDER),
                scroll_y: 0,
                layout: VmLayout::default(),
                cursor_addr: None,
                hex: VmHexView::new(),
                sub_focus: VmHexPaneFocus::Hex,
                jump: VmJumpState::new(),
            },
            detail: DetailView {
                lines: split_lines(DETAIL_PLACEHOLDER),
                scroll_y: 0,
            },
            debugger,
            binary_path,
            symbol_index,
            symbol_index_mode,
            warned_stale_binary: false,
            verbose,

            // Initialize popup state
            show_symbols_popup: false,
            last_main_focus: Focus::Source,

            // Initialize main split ratio
            main_split: 60,

            // Initialize symbols popup width
            symbols_popup_width: 40,
        }
    }

    /// Adjust the main split ratio between Source (left) and VM (right)
    pub fn adjust_main_split(&mut self, delta: i16) {
        let mut v = self.main_split as i16 + delta;
        // Clamp between 30% and 80% to avoid extreme squishing
        if v < 30 {
            v = 30;
        }
        if v > 80 {
            v = 80;
        }
        self.main_split = v as u16;
    }

    /// Adjust the Symbols popup width (in columns)
    pub fn adjust_symbols_popup_width(&mut self, delta: i16) {
        let mut v = self.symbols_popup_width as i16 + delta;
        // Clamp between 20 and 120 columns
        if v < 20 {
            v = 20;
        }
        if v > 120 {
            v = 120;
        }
        self.symbols_popup_width = v as u16;
    }

    /// Refresh the VM layout (used by the minimap) from the current inferior process
    pub fn refresh_vm_layout_from_session(&mut self) {
        // Try to get PID from gdb
        let pid = match self.debugger.inferior_pid() {
            Ok(pid) => pid,
            Err(err) => {
                // Log error but don't clear the layout - keep showing old data
                crate::logger::log_debug(&format!(
                    "[vm] failed to get inferior pid for VM layout: {:?}",
                    err
                ));
                return;
            }
        };

        // Try to read /proc/<pid>/maps
        match self.vm.layout.refresh_from_pid(pid) {
            Ok(()) => {
                if self.verbose {
                    crate::logger::log_debug(&format!(
                        "[vm] refreshed VM layout from pid {}: {} regions",
                        pid,
                        self.vm.layout.regions.len()
                    ));
                }
            }
            Err(err) => {
                crate::logger::log_debug(&format!(
                    "[vm] failed to read /proc/{}/maps for VM layout: {:?}",
                    pid, err
                ));
                // Keep old layout on error
            }
        }
    }

    /// Refresh TUI state after gdb stops (at breakpoint, step, etc.)
    pub fn refresh_after_stop(&mut self, stopped: Option<&StoppedLocation>) -> Result<()> {
        let t0 = Instant::now();
        // Prefer querying gdb for the freshest frame; fall back to provided stop info.
        let frame = match self.current_frame() {
            Ok(f) => f,
            Err(_) => match stopped {
                Some(loc) => FrameInfo {
                    func: loc.clone().func.unwrap_or_else(|| "<unknown>".to_string()),
                    file: loc.clone().file,
                    fullname: loc.clone().fullname,
                    line: loc.line,
                },
                None => return Ok(()),
            },
        };
        let t1 = Instant::now();

        self.update_source_view_from_frame(&frame)?;
        let t2 = Instant::now();
        self.update_symbols(&frame)?;
        let t3 = Instant::now();
        self.refresh_vm_layout_from_session();
        let t4 = Instant::now();

        if self.verbose {
            crate::logger::log_debug(&format!(
                "[tui] refresh_after_stop: frame={}ms, source={}ms, symbols={}ms, vm={}ms",
                (t1 - t0).as_millis(),
                (t2 - t1).as_millis(),
                (t3 - t2).as_millis(),
                (t4 - t3).as_millis()
            ));
        }

        Ok(())
    }

    /// Update symbols panel with current locals and globals
    fn update_symbols(&mut self, frame: &FrameInfo) -> Result<()> {
        // Read locals from current frame
        let locals = self.debugger.list_locals()?;
        self.symbols.locals = locals.into_iter().map(format_local_entry).collect();

        // Globals via symbol index only.
        let basename = frame
            .file
            .as_deref()
            .or(frame.fullname.as_deref())
            .and_then(|p| std::path::Path::new(p).file_name())
            .and_then(|os| os.to_str())
            .map(|s| s.to_owned());

        if self.symbol_index.is_none() {
            if let Ok(idx) = self
                .debugger
                .build_symbol_index(self.symbol_index_mode, basename.as_deref())
            {
                self.symbol_index = Some(idx);
            } else {
                crate::logger::log_debug("[tui] build_symbol_index failed; globals empty");
            }
        }

        if let (Some(ref idx), Some(file)) = (&self.symbol_index, basename.as_deref()) {
            let globals_with_vals: Vec<GlobalVarWithValue> =
                self.debugger.list_globals_from_index(idx, Some(file))?;
            crate::logger::log_debug(&format!(
                "[tui] globals_for_basename {} -> {} entries",
                file,
                globals_with_vals.len()
            ));
            self.symbols.globals = globals_with_vals
                .into_iter()
                .map(format_global_from_value)
                .collect();
        } else {
            crate::logger::log_debug(
                "[tui] no symbol_index or no frame basename; clearing globals",
            );
            self.symbols.globals.clear();
        }

        // Ensure selected_index is within bounds
        let max_index = match self.symbols.selected_section {
            SymbolSection::Locals => self.symbols.locals.len().saturating_sub(1),
            SymbolSection::Globals => self.symbols.globals.len().saturating_sub(1),
        };
        if self.symbols.selected_index > max_index {
            self.symbols.selected_index = 0;
        }

        // If locals is empty but globals is not, switch to globals
        if self.symbols.locals.is_empty()
            && !self.symbols.globals.is_empty()
            && matches!(self.symbols.selected_section, SymbolSection::Locals)
        {
            self.symbols.selected_section = SymbolSection::Globals;
            self.symbols.selected_index = 0;
        }

        Ok(())
    }

    /// Get current stack frame from gdb
    fn current_frame(&mut self) -> Result<FrameInfo> {
        // Use -stack-info-frame to get current frame
        let resp = self.debugger.exec_command("-stack-info-frame")?;

        // Parse frame info from response
        let func = parse_field(&resp.result, "func");
        let file = parse_field(&resp.result, "file");
        let fullname = parse_field(&resp.result, "fullname");
        let line = parse_field(&resp.result, "line").and_then(|s| s.parse::<u32>().ok());

        Ok(FrameInfo {
            func: func.unwrap_or_else(|| "<unknown>".to_string()),
            file,
            fullname,
            line,
        })
    }

    fn update_source_view_from_frame(&mut self, frame: &FrameInfo) -> Result<()> {
        let line = match frame.line {
            Some(l) => l,
            None => return Ok(()), // No line info, skip
        };

        // Prefer fullname (absolute path), fallback to file
        let path_str = frame
            .fullname
            .as_ref()
            .or_else(|| frame.file.as_ref())
            .cloned();

        let Some(path_str) = path_str else {
            return Ok(());
        };

        let path = PathBuf::from(path_str);

        // Reload file if changed or not loaded
        let need_reload = self.source.filename.as_ref() != Some(&path);
        if need_reload {
            let contents = std::fs::read_to_string(&path)?;
            self.source.lines = contents.lines().map(|s| s.to_string()).collect();
            self.source.filename = Some(path);
        }
        self.warn_if_source_newer();

        // gdb의 frame.line은 "다음에 실행될 소스 라인(PC)"을 가리킨다.
        // 따라서 ▶ 표시 줄은 아직 실행 전이며, locals/globals는 직전까지 실행된 상태를 보여준다.
        // 한 줄 늦어 보이는 것은 gdb 표준 semantics를 그대로 따른 결과다.
        self.source.current_line = Some(line);
        self.adjust_source_scroll(line);

        Ok(())
    }

    fn warn_if_source_newer(&mut self) {
        if self.warned_stale_binary {
            return;
        }
        let src_path = match &self.source.filename {
            Some(p) => p,
            None => return,
        };
        let src_mtime = std::fs::metadata(src_path).and_then(|m| m.modified()).ok();
        let bin_mtime = std::fs::metadata(&self.binary_path)
            .and_then(|m| m.modified())
            .ok();
        match (src_mtime, bin_mtime) {
            (Some(src), Some(bin)) => {
                if src > bin {
                    crate::logger::log_debug(&format!(
                        "[tui] warning: source file newer than executable ({} > {}), line info may be misaligned. Rebuild the target.",
                        fmt_time(src),
                        fmt_time(bin)
                    ));
                    self.warned_stale_binary = true;
                }
            }
            _ => {}
        }
    }

    fn adjust_source_scroll(&mut self, current_line: u32) {
        // current_line is 1-based, scroll_top is 0-based
        let idx = current_line.saturating_sub(1);

        // Keep the current line at the top of the view after a stop.
        self.source.scroll_top = idx;
    }

    /// 현재 VmHexView 설정(bytes_per_line, lines_per_page, top_addr)에 맞춰
    /// gdb를 통해 한 페이지 분량의 메모리를 읽어와 vm.hex.buf / valid_len 을 채운다.
    pub fn refresh_vm_hex_page(&mut self) -> anyhow::Result<()> {
        // lines_per_page 가 0 이면 아직 렌더링에서 설정되지 않은 상태이므로
        // 일단 아무 것도 하지 않고 Ok(()) 반환
        let lines = self.vm.hex.lines_per_page;
        if lines == 0 {
            return Ok(());
        }

        let bpl = self.vm.hex.bytes_per_line.max(1);
        let page_size = (lines as usize) * (bpl as usize);

        let addr = self.vm.hex.top_addr;

        // 이전 내용은 지워두고 시작
        self.vm.hex.buf.clear();
        self.vm.hex.valid_len = 0;

        crate::logger::log_debug(&format!(
            "[vm] refresh_vm_hex_page: addr=0x{:x}, size={}",
            addr, page_size
        ));

        match self.debugger.read_memory_bytes(addr, page_size) {
            Ok(bytes) => {
                self.vm.hex.buf = bytes;
                self.vm.hex.valid_len = self.vm.hex.buf.len();
            }
            Err(e) => {
                crate::logger::log_debug(&format!(
                    "[vm] memory read failed at 0x{:x}: {:?}",
                    addr, e
                ));
                // 실패해도 페이지 크기만큼 0으로 채워서 화면이 “바뀌었다”는 걸 보장
                self.vm.hex.buf = vec![0u8; page_size];
                self.vm.hex.valid_len = 0;
            }
        }

        // 어떤 경우든 "이 주소/라인 수로 로드했다"는 사실은 기록해 둔다.
        self.vm.hex.last_loaded_top_addr = self.vm.hex.top_addr;
        self.vm.hex.last_loaded_lines_per_page = self.vm.hex.lines_per_page;

        Ok(())
    }

    // === VM hexdump navigation ===

    fn vm_bpl(&self) -> u64 {
        self.vm.hex.bytes_per_line as u64
    }

    fn vm_page_size(&self) -> u64 {
        self.vm_bpl() * self.vm.hex.lines_per_page as u64
    }

    fn debug_assert_vm_invariants(&self) {
        let bpl = self.vm_bpl();
        let page_size = self.vm_page_size();

        debug_assert!(self.vm.hex.lines_per_page > 0);
        debug_assert!(bpl > 0);

        let start = self.vm.hex.top_addr;
        let end = start + page_size;

        debug_assert!(
            self.vm.hex.cursor_addr >= start && self.vm.hex.cursor_addr < end
        );
    }

    pub fn vm_move_up(&mut self) {
        if self.vm.hex.lines_per_page == 0 || self.vm.hex.bytes_per_line == 0 {
            return;
        }
        let bpl = self.vm_bpl();
        let rows = self.vm.hex.lines_per_page as u64;

        let mut top = self.vm.hex.top_addr;
        let cursor = self.vm.hex.cursor_addr;
        let offset = cursor.saturating_sub(top);
        let mut row = (offset / bpl).min(rows.saturating_sub(1));
        let col = offset % bpl;

        if row > 0 {
            row -= 1;
        } else {
            top = top.saturating_sub(bpl);
            row = 0;
        }

        self.vm.hex.top_addr = top;
        self.vm.hex.cursor_addr = top + row * bpl + col;

        self.debug_assert_vm_invariants();
    }

    pub fn vm_move_down(&mut self) {
        if self.vm.hex.lines_per_page == 0 || self.vm.hex.bytes_per_line == 0 {
            return;
        }
        let bpl = self.vm_bpl();
        let rows = self.vm.hex.lines_per_page as u64;

        let mut top = self.vm.hex.top_addr;
        let cursor = self.vm.hex.cursor_addr;
        let offset = cursor.saturating_sub(top);
        let mut row = (offset / bpl).min(rows.saturating_sub(1));
        let col = offset % bpl;

        if row + 1 < rows {
            row += 1;
        } else {
            top = top.saturating_add(bpl);
            row = rows.saturating_sub(1);
        }

        self.vm.hex.top_addr = top;
        self.vm.hex.cursor_addr = top + row * bpl + col;

        self.debug_assert_vm_invariants();
    }

    fn vm_move_byte(&mut self, dir: i64) {
        if self.vm.hex.lines_per_page == 0 || self.vm.hex.bytes_per_line == 0 {
            return;
        }
        let bpl = self.vm_bpl();
        let rows = self.vm.hex.lines_per_page as u64;

        let mut top = self.vm.hex.top_addr;
        let cursor = self.vm.hex.cursor_addr;
        let offset = cursor.saturating_sub(top);
        let mut row = (offset / bpl).min(rows.saturating_sub(1));
        let mut col = offset % bpl;

        let mut new_row = row as i128;
        let mut new_col = col as i128 + dir as i128;
        let bpl_i = bpl as i128;

        while new_col < 0 {
            new_col += bpl_i;
            new_row -= 1;
        }
        while new_col >= bpl_i {
            new_col -= bpl_i;
            new_row += 1;
        }

        if new_row < 0 {
            let delta_rows = (-new_row) as u64;
            top = top.saturating_sub(delta_rows * bpl);
            new_row = 0;
        } else if new_row >= rows as i128 {
            let delta_rows = (new_row as u64).saturating_sub(rows.saturating_sub(1));
            top = top.saturating_add(delta_rows * bpl);
            new_row = rows.saturating_sub(1) as i128;
        }

        row = new_row as u64;
        col = new_col as u64;

        self.vm.hex.top_addr = top;
        self.vm.hex.cursor_addr = top + row * bpl + col;

        self.debug_assert_vm_invariants();
    }

    pub fn vm_left(&mut self) {
        match self.vm.sub_focus {
            VmHexPaneFocus::Hex => {
                if let Some((_row, col)) = self.vm.hex.cursor_row_col() {
                    if col > 0 {
                        self.vm_move_byte(-1);
                    } else {
                        // Hex 첫 컬럼에서 ← → Address 패널로 포커스만 이동
                        self.vm.sub_focus = VmHexPaneFocus::Address;
                    }
                }
            }
            VmHexPaneFocus::Address => {
                // 왼쪽으로 더 갈 패널이 없으므로 아무 것도 하지 않음
            }
        }
    }

    pub fn vm_right(&mut self) {
        match self.vm.sub_focus {
            VmHexPaneFocus::Hex => {
                // 바이트 하나 오른쪽으로 이동 (페이지 넘으면 vm_move_byte가 처리)
                self.vm_move_byte(1);
            }
            VmHexPaneFocus::Address => {
                // Address → Hex 첫 컬럼으로 포커스 이동
                self.vm.sub_focus = VmHexPaneFocus::Hex;
                if let Some((row, _col)) = self.vm.hex.cursor_row_col() {
                    self.vm.hex.cursor_addr = self.vm.hex.addr_from_row_col(row, 0);
                }
            }
        }
    }

    pub fn vm_page_up(&mut self) {
        if self.vm.hex.lines_per_page == 0 || self.vm.hex.bytes_per_line == 0 {
            return;
        }
        let bpl = self.vm_bpl();
        let rows = self.vm.hex.lines_per_page as u64;
        let page_size = self.vm_page_size();
        if page_size == 0 {
            return;
        }

        let start = self.vm.hex.top_addr;
        let cursor = self.vm.hex.cursor_addr;
        let offset = cursor.saturating_sub(start);
        let row = (offset / bpl).min(rows.saturating_sub(1));
        let col = offset % bpl;

        self.vm.hex.top_addr = self.vm.hex.top_addr.saturating_sub(page_size);
        let top = self.vm.hex.top_addr;
        self.vm.hex.cursor_addr = top + row * bpl + col;

        self.debug_assert_vm_invariants();
    }

    pub fn vm_page_down(&mut self) {
        if self.vm.hex.lines_per_page == 0 || self.vm.hex.bytes_per_line == 0 {
            return;
        }
        let bpl = self.vm_bpl();
        let rows = self.vm.hex.lines_per_page as u64;
        let page_size = self.vm_page_size();
        if page_size == 0 {
            return;
        }

        let start = self.vm.hex.top_addr;
        let cursor = self.vm.hex.cursor_addr;
        let offset = cursor.saturating_sub(start);
        let row = (offset / bpl).min(rows.saturating_sub(1));
        let col = offset % bpl;

        self.vm.hex.top_addr = self.vm.hex.top_addr.saturating_add(page_size);
        let top = self.vm.hex.top_addr;
        self.vm.hex.cursor_addr = top + row * bpl + col;

        self.debug_assert_vm_invariants();
    }

    // === VM Jump Mode ===

    pub fn vm_jump_start(&mut self) {
        self.vm.jump.active = true;
        self.vm.jump.clear();
    }

    pub fn vm_jump_cancel(&mut self) {
        self.vm.jump.active = false;
        self.vm.jump.clear();
    }

    pub fn vm_jump_push_char(&mut self, ch: char) {
        if !self.vm.jump.active {
            return;
        }
        // 16진수 문자와 'x' / 'X'만 허용 (0x 접두사 허용용)
        if ch.is_ascii_hexdigit() || ch == 'x' || ch == 'X' {
            self.vm.jump.input.push(ch);
            self.vm.jump.error = None;
        }
    }

    pub fn vm_jump_backspace(&mut self) {
        if !self.vm.jump.active {
            return;
        }
        self.vm.jump.input.pop();
        self.vm.jump.error = None;
    }

    pub fn vm_jump_confirm(&mut self) {
        if !self.vm.jump.active {
            return;
        }

        let raw = self.vm.jump.input.trim();
        if raw.is_empty() {
            self.vm.jump.error = Some("empty address".to_string());
            return;
        }

        // 0x 접두사 제거
        let s = raw
            .strip_prefix("0x")
            .or_else(|| raw.strip_prefix("0X"))
            .unwrap_or(raw);

        let addr = match u64::from_str_radix(s, 16) {
            Ok(v) => v,
            Err(_) => {
                self.vm.jump.error = Some("invalid hex address".to_string());
                return;
            }
        };

        // VM 레이아웃 범위 체크
        if let Some((min_addr, max_addr)) = self.vm.layout.addr_range() {
            if addr < min_addr || addr >= max_addr {
                self.vm.jump.error = Some("address outside known VM range".to_string());
                return;
            }
        }

        // hexdump 페이지/커서 이동
        if self.vm.hex.bytes_per_line == 0 || self.vm.hex.lines_per_page == 0 {
            // 아직 렌더 전에 jump를 호출한 경우 – 일단 커서만 맞춰두고 종료
            self.vm.hex.cursor_addr = addr;
        } else {
            let bpl = self.vm.hex.bytes_per_line as u64;
            let lines = self.vm.hex.lines_per_page as u64;
            let page_size = bpl * lines;

            // 주소를 줄 시작으로 맞춤
            let line_start = addr / bpl * bpl;

            // 타겟 줄을 화면 중간쯤에 두고 싶으면 offset 줄 만큼 위로 빼기
            let center_offset_lines = lines / 2;
            let mut top = line_start.saturating_sub(center_offset_lines * bpl);

            // 최대 주소 기준으로 top을 클램프
            if let Some((_min_addr, max_addr)) = self.vm.layout.addr_range() {
                if max_addr > page_size {
                    let max_top = max_addr - page_size;
                    if top > max_top {
                        top = max_top;
                    }
                }
            }

            self.vm.hex.top_addr = top;
            self.vm.hex.cursor_addr = addr;

            // 실제 페이지 읽기
            let _ = self.refresh_vm_hex_page();
        }

        // 서브 포커스는 Hex로 맞춰 둔다
        self.vm.sub_focus = VmHexPaneFocus::Hex;

        // 점프 모드 종료
        self.vm.jump.active = false;
    }
}

/// Minimal frame info for source view
struct FrameInfo {
    func: String,
    file: Option<String>,
    fullname: Option<String>,
    line: Option<u32>,
}

/// Simple field parser helper
fn parse_field(s: &str, key: &str) -> Option<String> {
    let pattern = format!("{}=\"", key);
    if let Some(start) = s.find(&pattern) {
        let start = start + pattern.len();
        if let Some(end) = s[start..].find('"') {
            return Some(s[start..start + end].to_string());
        }
    }
    None
}

fn split_lines(s: &str) -> Vec<String> {
    s.lines().map(|l| l.to_string()).collect()
}

fn fmt_time(t: SystemTime) -> String {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => format!("{}", d.as_secs()),
        Err(_) => "unknown".to_string(),
    }
}

/// Format LocalVar into SymbolEntry
fn format_local_entry(var: LocalVar) -> SymbolEntry {
    let value = var
        .value
        .as_ref()
        .map(|v| prettify_value(v))
        .unwrap_or_else(|| "<unavailable>".to_string());

    let type_name = var.ty.as_deref().unwrap_or("unknown");
    let normalized_type = normalize_display_type(type_name);

    SymbolEntry {
        name: var.name.clone(),
        type_name: type_name.to_string(),
        value_preview: format!("{} {} = {}", normalized_type, var.name, value),
    }
}

/// Format GlobalVar into SymbolEntry
fn format_global_entry(var: GlobalVar) -> SymbolEntry {
    let value = prettify_value(&var.value);
    let normalized_type = normalize_display_type(&var.type_name);

    SymbolEntry {
        name: var.name.clone(),
        type_name: var.type_name.clone(),
        value_preview: format!("{} {} = {}", normalized_type, var.name, value),
    }
}

fn format_global_from_value(g: GlobalVarWithValue) -> SymbolEntry {
    let ty = g
        .info
        .type_name
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let normalized_type = normalize_display_type(&ty);
    SymbolEntry {
        name: g.info.name.clone(),
        type_name: ty.clone(),
        value_preview: format!(
            "{} {} = {}",
            normalized_type,
            g.info.name,
            prettify_value(&g.value)
        ),
    }
}

/// Normalize type name for display (same logic as printers.rs)
fn normalize_display_type(ty: &str) -> String {
    if ty.contains('*') {
        normalize_pointer_type(ty)
    } else {
        normalize_type_name(ty)
    }
}
