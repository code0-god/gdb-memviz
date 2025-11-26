mod interactive;
mod mi;

use mi::{MiResponse, MiSession, MiStatus, Result};

fn main() -> Result<()> {
    let mut gdb_bin = std::env::var("GDB").unwrap_or_else(|_| "gdb".to_string());
    let mut verbose = false;
    let mut target: Option<String> = None;
    let mut target_args: Vec<String> = Vec::new();

    let mut iter = std::env::args().skip(1).peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--gdb" => {
                if let Some(bin) = iter.next() {
                    gdb_bin = bin;
                } else {
                    eprintln!("usage: cargo run -- [--verbose|-v] [--gdb <gdb-path>] <target> [args]");
                    std::process::exit(1);
                }
            }
            "--verbose" | "-v" => {
                verbose = true;
            }
            _ => {
                target = Some(arg);
                target_args.extend(iter);
                break;
            }
        }
    }

    if target.is_none() {
        eprintln!("usage: cargo run -- [--verbose|-v] [--gdb <gdb-path>] <target> [args]");
        std::process::exit(1);
    }
    let target = target.unwrap();
    if !std::path::Path::new(&target).exists() {
        eprintln!("target not found: {}", target);
        std::process::exit(1);
    }

    println!(
        "[gdb-memviz] gdb: {} | target: {} {:?} | verbose: {}",
        gdb_bin, target, target_args, verbose
    );
    let mut session = MiSession::start(&gdb_bin, &target, &target_args, verbose)?;
    session.drain_initial_output()?;

    println!("\n# probing gdb");
    describe_response("version", &session.exec_command("-gdb-version")?);
    describe_response("features", &session.exec_command("-list-features")?);

    println!("\n# break main and run");
    session.run_to_main()?;
    println!("Reached breakpoint at main. Type 'help' for commands.");

    interactive::repl(&mut session)?;
    session.shutdown();
    Ok(())
}

fn describe_response(label: &str, resp: &MiResponse) {
    match &resp.status {
        MiStatus::Done | MiStatus::Running => {
            println!("[{}] {}", label, resp.result);
        }
        MiStatus::Error(msg) => {
            println!("[{}] error: {}", label, msg);
        }
        MiStatus::Other(msg) => {
            println!("[{}] {}", label, msg);
        }
    }
    for line in &resp.oob {
        println!("  {}", line);
    }
}
