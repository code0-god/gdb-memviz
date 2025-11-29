// Entry point wires CLI parsing to the MI session and REPL.
mod interactive;
mod logger;
mod mi;
mod symbols;
mod tui;
mod types;
mod vm;

use logger::log_debug;
use mi::{MiResponse, MiSession, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use symbols::SymbolIndexMode;

enum TargetKind {
    Binary { path: PathBuf, args: Vec<String> },
    SingleSource { path: PathBuf, args: Vec<String> },
}

fn main() -> Result<()> {
    // Parse CLI: allow --gdb override, verbose MI logging, log file, and forward the remaining args
    // to the target binary or single source. Exits with usage on missing target.
    let usage = "usage: cargo run -- [--verbose|-v] [--log-file <path>] [--gdb <gdb-path>] [--symbol-index-mode <mode>] [--tui|-t] <target> [args]";
    let mut gdb_bin = std::env::var("GDB").unwrap_or_else(|_| "gdb".to_string());
    let mut verbose = false;
    let mut tui_mode = false;
    let mut log_file: Option<PathBuf> = None;
    let mut symbol_index_mode = SymbolIndexMode::DebugOnly;
    let mut target: Option<String> = None;
    let mut target_args: Vec<String> = Vec::new();

    // Simple flag parser: stops at first non-flag and treats the rest as program+args.
    let mut iter = std::env::args().skip(1).peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--gdb" => {
                if let Some(bin) = iter.next() {
                    gdb_bin = bin;
                } else {
                    eprintln!("{}", usage);
                    std::process::exit(1);
                }
            }
            "--verbose" | "-v" => {
                verbose = true;
            }
            "--tui" | "-t" => {
                tui_mode = true;
            }
            "--log-file" => {
                if let Some(path) = iter.next() {
                    log_file = Some(PathBuf::from(path));
                } else {
                    eprintln!("{}", usage);
                    std::process::exit(1);
                }
            }
            "--symbol-index-mode" => {
                if let Some(mode) = iter.next() {
                    symbol_index_mode = match mode.as_str() {
                        "none" => SymbolIndexMode::None,
                        "debug-only" => SymbolIndexMode::DebugOnly,
                        "debug-and-nondebug" => SymbolIndexMode::DebugAndNonDebug,
                        _ => {
                            eprintln!(
                                "invalid --symbol-index-mode '{}', expected one of: none, debug-only, debug-and-nondebug",
                                mode
                            );
                            std::process::exit(1);
                        }
                    };
                } else {
                    eprintln!("{}", usage);
                    std::process::exit(1);
                }
            }
            _ => {
                target = Some(arg);
                target_args.extend(iter);
                break;
            }
        }
    }

    let target = match target {
        Some(t) => t,
        None => {
            eprintln!("{}", usage);
            std::process::exit(1);
        }
    };

    // Initialize logger (file only).
    if verbose || log_file.is_some() {
        let path = log_file.unwrap_or_else(|| {
            let mut p = std::env::temp_dir();
            p.push(format!("gdb-memviz-{}.log", std::process::id()));
            p
        });
        if let Err(e) = logger::global().init(&path, verbose) {
            eprintln!("failed to init logger {}: {}", path.display(), e);
        }
    } else {
        let _ = logger::global().init(std::env::temp_dir().join("gdb-memviz.log"), false);
    }

    let target_path = PathBuf::from(&target);
    if !target_path.exists() {
        eprintln!("target not found: {}", target);
        std::process::exit(1);
    }

    let target_kind = if is_source_file(&target_path) {
        TargetKind::SingleSource {
            path: target_path,
            args: target_args.clone(),
        }
    } else {
        TargetKind::Binary {
            path: target_path,
            args: target_args.clone(),
        }
    };

    let target_basename = match target_kind {
        TargetKind::SingleSource { ref path, .. } => path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string()),
        _ => None,
    };

    let (bin_path, prog_args) = match target_kind {
        TargetKind::Binary { path, args } => (path, args),
        TargetKind::SingleSource { path, args } => {
            let out = compile_single_source(&path, verbose)?;
            (out, args)
        }
    };

    let bin_str = bin_path
        .to_str()
        .ok_or_else(|| "binary path is not valid UTF-8".to_string())?;

    if tui_mode {
        return tui::run_tui(
            &gdb_bin,
            bin_str,
            &prog_args,
            verbose,
            symbol_index_mode,
            target_basename.clone(),
        );
    }

    log_debug(&format!(
        "[gdb-memviz] gdb: {} | target: {} {:?} | verbose: {}",
        gdb_bin, bin_str, prog_args, verbose
    ));
    // Launch gdb/MI and do one-time probing before entering the REPL.
    let mut session = MiSession::start(
        &gdb_bin,
        bin_str,
        &prog_args,
        verbose,
        symbol_index_mode,
        target_basename.clone(),
    )?;
    session.drain_initial_output()?;

    log_debug("# probing gdb");
    let version = session.exec_command("-gdb-version")?;
    let features = session.exec_command("-list-features")?;
    describe_response("version", &version, verbose);
    describe_response("features", &features, verbose);

    log_debug("# break main and run");
    session.run_to_main()?;
    session.ensure_word_size();
    session.ensure_arch();
    session.ensure_endian();
    // Build symbol index once (best effort)
    if let Err(e) = session.build_symbol_index(symbol_index_mode, target_basename.as_deref()) {
        log_debug(&format!("[sym] build_symbol_index (CLI) failed: {:?}", e));
    }
    log_debug("Reached breakpoint at main. Type 'help' for commands.");

    interactive::repl(&mut session)?;
    session.shutdown();
    Ok(())
}

fn is_source_file(p: &Path) -> bool {
    match p.extension().and_then(|s| s.to_str()) {
        Some(ext) => matches!(ext, "c" | "cc" | "cpp" | "cxx"),
        None => false,
    }
}

fn compile_single_source(path: &Path, verbose: bool) -> Result<PathBuf> {
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let mut out = path.to_path_buf();
    let stem = out.file_stem().and_then(|s| s.to_str()).unwrap_or("a.out");
    out.set_file_name(format!("{}-memviz.out", stem));

    if verbose {
        log_debug(&format!(
            "[build] compiling single source with {} -> {}",
            cc,
            out.display()
        ));
    }

    let status = Command::new(cc)
        .arg("-g")
        .arg("-O0")
        .arg("-fno-omit-frame-pointer")
        .arg(path)
        .arg("-o")
        .arg(&out)
        .status()?;

    if !status.success() {
        return Err(format!("failed to compile {:?} (status: {status})", path).into());
    }

    Ok(out)
}

/// Helper to echo MI responses when verbose is enabled.
fn describe_response(label: &str, resp: &MiResponse, verbose: bool) {
    if !verbose {
        return;
    }
    log_debug(&format!("[{}] {}", label, resp.result));
    for line in &resp.oob {
        log_debug(&format!("  {}", line));
    }
}
