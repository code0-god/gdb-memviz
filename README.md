<h1 align="center">gdb-memviz</h1>

<p align="center">
  Experimental TUI/CLI tool to visualize C/C++ program memory on top of <code>gdb</code>/MI.
</p>

<p align="center">
  <!-- Badges (replace <user> if you rename the repo) -->
  <img src="https://img.shields.io/github/last-commit/code0-god/gdb-memviz?style=flat&logo=git&logoColor=white&color=0080ff" alt="last-commit">
  <img src="https://img.shields.io/github/languages/top/code0-god/gdb-memviz?style=flat&color=0080ff" alt="top-language">
  <img src="https://img.shields.io/github/license/code0-god/gdb-memviz?style=flat&color=0080ff" alt="license">
</p>

---

> **Status**: early prototype.  
> API, UI, and keymaps may change at any time.

`gdb-memviz` is a small experiment to answer:

> “What if gdb could *show* how memory is laid out, instead of just dumping hex?”

It runs `gdb` in **MI mode**, probes the target process, and then presents:

- locals / globals
- type layouts (struct/array)
- raw memory dumps
- VM regions (text / data / heap / stack)

…in a text-based TUI or REPL-style CLI.

The goal is to make **systems programming & debugging** a bit less abstract, especially for students who struggle to connect source code with actual memory layout.

---

## Features

### Core

- Runs `gdb` in MI mode, automatically:
  - loads the target (binary or single `.c/.cc/.cpp/.cxx` file)
  - sets a breakpoint at `main`
- Basic debugging control:
  - `break` / `b`
  - `next` / `n` (step over)
  - `step` / `s` (step into – CLI)
  - `continue` / `c`
- Locals & globals
  - `locals`: current frame locals (name / type / value)
  - `globals`: global & static variables from the executable
- Memory-oriented commands
  - `mem <expr> [len]`  
    Read memory starting from `&<expr>`:
    - length = `sizeof(<expr>)` by default (capped at 512B)
    - or an explicit `len`
    - shows hex + ASCII + basic header (arch / endian / type)
  - `view <symbol>`  
    Show a type layout for structs/arrays (offset/size per field) plus raw dump.
  - `follow <symbol> [depth]`  
    Follow a pointer chain through `next` (or the first pointer field) up to `depth`.
- VM view
  - `vm`: summarize `/proc/<pid>/maps` into text/data/heap/stack/lib/anon regions
  - `vm locate <expr>`: find which region a symbol/address lives in
  - `vm vars`: group locals/globals/heap objects by VM region

### Symbol index (globals)

Global symbols are collected using `-symbol-info-variables` and cached:

- `--symbol-index-mode debug-only` (default)  
  Only debug symbols, optimized for speed.
- `--symbol-index-mode debug-and-nondebug`  
  Includes libc and everything else (slow on some systems).
- `--symbol-index-mode none`  
  Skip the symbol index (no globals panel, faster startup).

Single-source mode (passing `examples/sample.c` instead of a binary) will:

- auto-compile with `-g` (no optimization)
- only parse symbol info for the target `basename` to avoid glibc spam

### TUI (experimental)

The TUI is inspired by Neovim / modern terminal apps.

Layout:

- **Header bar**  
  Shows mode (`NORMAL`), current focus, file name, arch, symbol-index mode, and key hints.
- **Main area**
  - Left: **Source** panel
    - actual source file
    - simple C/C++ syntax highlighting (keywords, types, strings, numbers, comments)
    - statusline-style tab with `sample.c:37` etc.
    - current PC line highlighted across full width
  - Right: **VM Layout** panel
    - placeholder VM canvas for now
    - colored bars for `[stack]`, `[heap]`, `[data]`, `[text]` regions
- **Floating Symbols popup**
  - `locals` / `globals` sections
  - values rendered with simple type-aware styling (numbers, pointers, strings)
  - behaves like a floating “quick info” card over the Source pane
- **Command line**
  - Neovim-style `:` line at the bottom (for future command mode)

Keymaps (default, subject to change):

- Focus / panels  
  - `Ctrl+h` (or `Ctrl+a`*) : focus Source  
  - `Ctrl+l` (or `Ctrl+d`*) : focus VM  
  - `Ctrl+s`              : open Symbols popup + focus it  
  - `Esc`                 : close popup, return to previous focus  
  - `*` = planned / easy to add via `src/tui/keymap.rs`
- Layout
  - `Ctrl+←` / `Ctrl+→`   : adjust Source/VM split (30%–80%)  
  - When Symbols is focused: same keys resize popup width
- Scrolling
  - `↑` / `↓`             : scroll focused panel / move selection  
  - `PageUp` / `PageDown` : bigger scroll
  - In Symbols:  
    - `l` : jump to locals  
    - `g` : jump to globals
- Debugging
  - `n`                   : step over (gdb `next`)
- Quit
  - `q` or `Ctrl+c`

All bindings are centralized in [`src/tui/keymap.rs`], and the header bar automatically shows the important ones.

---

## Requirements

- **Linux** (uses `/proc`, tested on x86_64 / aarch64)
- **gdb** ≥ 8.x  
  - must support `-gdb-show endian` and MI commands like `-symbol-info-variables`
- **Rust** stable (for building with `cargo`)
- A terminal that supports:
  - 256 colors
  - basic ANSI escape sequences

---

## Install & Build

Clone and build from source:

```bash
git clone https://github.com/code0-god/gdb-memviz.git
cd gdb-memviz

# Build the Rust binary
cargo build
````

No installation step is strictly required; you can run via `cargo run` or `target/debug/gdb-memviz`.

---

## Quick start

### 1. Example program

Build the bundled example:

```bash
gcc -g examples/sample.c -o examples/sample
```

### 2. Run TUI

**Single-source mode** (auto-compile + TUI):

```bash
# default: symbol-index-mode=debug-only, logs to /tmp/gdb-memviz.log
cargo run -- --tui examples/sample.c
```

**Binary mode**:

```bash
cargo run -- --tui ./examples/sample
```

Optional flags:

```bash
# Symbol index behaviour
--symbol-index-mode debug-only        # default, recommended
--symbol-index-mode debug-and-nondebug # includes libc (slow)
--symbol-index-mode none              # skip globals index

# Logging
--log-file perf.log                   # write MI/TUI logs here
--verbose                             # mirror logs to stdout as well

# Custom gdb binary
--gdb /usr/bin/gdb
```

### 3. CLI / REPL mode

You can also run without the TUI and just use the REPL:

```bash
cargo run -- --symbol-index-mode debug-only --log-file perf.log examples/sample.c
```

Then try commands like:

```text
memviz> locals
memviz> globals
memviz> mem node
memviz> view node
memviz> follow node_ptr
memviz> vm
memviz> vm vars
memviz> vm locate pad
memviz> next
memviz> quit
```

---

## Project layout

```text
gdb-memviz/
├─ examples/
│  └─ sample.c        # sample C program used in docs & screenshots
└─ src/
   ├─ main.rs         # CLI/TUI entrypoint, argument parsing
   ├─ logger.rs       # simple file logger for MI/TUI tracing
   ├─ vm.rs           # /proc/<pid>/maps parsing, VM region model
   ├─ types.rs        # struct/array layout parsing (for view)
   ├─ interactive/    # CLI (REPL) commands: locals, mem, view, vm, follow, ...
   ├─ mi/             # gdb/MI session, parser, models, symbol index
   └─ tui/
      ├─ mod.rs       # TUI event loop, wiring to MiSession/AppState
      ├─ state.rs     # AppState (panes, focus, scroll, source buffer, symbols, ...)
      ├─ ui.rs        # layout + rendering (Source, Symbols, VM canvas, header, cmdline)
      ├─ theme.rs     # dark theme palette, borders, panel/card styles
      ├─ highlight.rs # lightweight C/C++ syntax highlighting
      └─ keymap.rs    # global/context key bindings + status bar hints
```

---

## Roadmap (rough)

* **TUI Phase T1**

  * Wire CLI `locals` / `globals` / `vm` data into TUI panels
  * Show live-updating locals/globals when stepping
* **TUI Phase T2**

  * VM canvas that places objects (stack frames, globals, heap nodes) in the layout
  * Highlight clicked/selected symbol across Source / Symbols / VM
* **TUI Phase T3**

  * Breakpoint list, inline breakpoint markers in Source
  * Command-line mode (`:`) for arbitrary MI/CLI commands
* Better type visualizations

  * struct/array boundaries, padding, alignment
  * integer/float interpretations in memory dumps
* Plugin / script hooks for teaching / demos

If you try this and have ideas—especially around teaching systems programming with visual memory views—issues and PRs are very welcome.

---

## License

This project is currently licensed under the MIT license.
See [`LICENSE`](./LICENSE) for details.