use crate::mi::{MiSession, Result};
use crate::tui::theme::Theme;
use std::path::PathBuf;
use std::time::SystemTime;

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

#[derive(Clone, Debug)]
pub struct SymbolsView {
    pub lines: Vec<String>,
    pub selected: usize,
    pub scroll_y: u16,
}

#[derive(Clone, Debug)]
pub struct VmView {
    pub lines: Vec<String>,
    pub scroll_y: u16,
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
    pub symbols: SymbolsView,
    pub vm: VmView,
    pub detail: DetailView,
    pub debugger: MiSession,
    pub binary_path: PathBuf,
    warned_stale_binary: bool,
}

impl AppState {
    pub fn new(debugger: MiSession, binary_path: PathBuf) -> Self {
        Self {
            theme: Theme::default(),
            focus: Focus::Source,
            layout: LayoutState::default(),
            source: SourceViewState::new(),
            symbols: SymbolsView {
                lines: split_lines(SYMBOLS_PLACEHOLDER),
                selected: 0,
                scroll_y: 0,
            },
            vm: VmView {
                lines: split_lines(VM_LAYOUT_PLACEHOLDER),
                scroll_y: 0,
            },
            detail: DetailView {
                lines: split_lines(DETAIL_PLACEHOLDER),
                scroll_y: 0,
            },
            debugger,
            binary_path,
            warned_stale_binary: false,
        }
    }

    /// Refresh TUI state after gdb stops (at breakpoint, step, etc.)
    pub fn refresh_after_stop(&mut self) -> Result<()> {
        // Get current frame information
        let frame = self.current_frame()?;

        // Update SourceView
        self.update_source_view_from_frame(&frame)?;

        // TODO: Update locals/globals/vm in later steps

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
                    eprintln!(
                        "[tui] warning: source file newer than executable ({} > {}), line info may be misaligned. Rebuild the target.",
                        fmt_time(src),
                        fmt_time(bin)
                    );
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
