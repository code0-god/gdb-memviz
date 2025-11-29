// Entry point wires CLI parsing to the MI session and REPL.
mod interactive;
mod mi;
mod tui;
mod types;
mod vm;

use mi::{MiResponse, MiSession, Result};

fn main() -> Result<()> {
    // Parse CLI: allow --gdb override, verbose MI logging, and forward the remaining args
    // to the target binary. Exits with usage on missing target.
    let usage = "usage: cargo run -- [--verbose|-v] [--gdb <gdb-path>] [--tui|-t] <target> [args]";
    let mut gdb_bin = std::env::var("GDB").unwrap_or_else(|_| "gdb".to_string());
    let mut verbose = false;
    let mut tui_mode = false;
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
            _ => {
                target = Some(arg);
                target_args.extend(iter);
                break;
            }
        }
    }

    if target.is_none() {
        eprintln!("{}", usage);
        std::process::exit(1);
    }
    let target = target.unwrap();
    if !std::path::Path::new(&target).exists() {
        eprintln!("target not found: {}", target);
        std::process::exit(1);
    }

    if tui_mode {
        return tui::run_tui(&gdb_bin, &target, &target_args, verbose);
    }

    println!(
        "[gdb-memviz] gdb: {} | target: {} {:?} | verbose: {}",
        gdb_bin, target, target_args, verbose
    );
    // Launch gdb/MI and do one-time probing before entering the REPL.
    let mut session = MiSession::start(&gdb_bin, &target, &target_args, verbose)?;
    session.drain_initial_output()?;

    println!("\n# probing gdb");
    let version = session.exec_command("-gdb-version")?;
    let features = session.exec_command("-list-features")?;
    describe_response("version", &version, verbose);
    describe_response("features", &features, verbose);

    println!("\n# break main and run");
    session.run_to_main()?;
    session.ensure_word_size();
    session.ensure_arch();
    session.ensure_endian();
    println!("Reached breakpoint at main. Type 'help' for commands.");

    interactive::repl(&mut session)?;
    session.shutdown();
    Ok(())
}

/// Helper to echo MI responses when verbose is enabled.
fn describe_response(label: &str, resp: &MiResponse, verbose: bool) {
    if !verbose {
        return;
    }
    eprintln!("[{}] {}", label, resp.result);
    for line in &resp.oob {
        eprintln!("  {}", line);
    }
}
