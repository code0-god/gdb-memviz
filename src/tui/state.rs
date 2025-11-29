use crate::tui::theme::Theme;

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
pub struct SourceView {
    pub lines: Vec<String>,
    pub scroll_y: u16,
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

#[derive(Clone, Debug)]
pub struct AppState {
    pub theme: Theme,
    pub focus: Focus,
    pub layout: LayoutState,

    pub source: SourceView,
    pub symbols: SymbolsView,
    pub vm: VmView,
    pub detail: DetailView,
}

impl AppState {
    pub fn new(
        source_lines: Vec<String>,
        symbols_lines: Vec<String>,
        vm_lines: Vec<String>,
        detail_lines: Vec<String>,
    ) -> Self {
        Self {
            theme: Theme::default(),
            focus: Focus::Source,
            layout: LayoutState::default(),
            source: SourceView {
                lines: source_lines,
                scroll_y: 0,
            },
            symbols: SymbolsView {
                lines: symbols_lines,
                selected: 0,
                scroll_y: 0,
            },
            vm: VmView {
                lines: vm_lines,
                scroll_y: 0,
            },
            detail: DetailView {
                lines: detail_lines,
                scroll_y: 0,
            },
        }
    }

    pub fn placeholder() -> Self {
        Self::new(
            split_lines(SOURCE_PLACEHOLDER),
            split_lines(SYMBOLS_PLACEHOLDER),
            split_lines(VM_LAYOUT_PLACEHOLDER),
            split_lines(DETAIL_PLACEHOLDER),
        )
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::placeholder()
    }
}

fn split_lines(s: &str) -> Vec<String> {
    s.lines().map(|l| l.to_string()).collect()
}
