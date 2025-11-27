mod commands;
mod follow;
mod printers;

use commands::{execute_command, CommandOutcome};
use crate::mi::{MiSession, Result};
use std::io::{self, Write};

pub fn repl(session: &mut MiSession) -> Result<()> {
    // Tiny read-eval-print loop: parse first token as command, rest as args, keep running
    // until EOF or quit.
    println!("Commands: locals | globals | mem <expr> [len] | view <symbol> | follow <symbol> [depth] | vm [locate <symbol>] | break <loc> | next | step | continue | help | quit");
    let stdin = io::stdin();
    let mut line = String::new();
    loop {
        print!("memviz> ");
        io::stdout().flush()?;
        line.clear();
        if stdin.read_line(&mut line)? == 0 {
            println!();
            break;
        }
        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        let mut parts = input.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or("").trim();
        let rest = parts.next().unwrap_or("").trim();
        match execute_command(input, cmd, rest, session) {
            Ok(CommandOutcome::Quit) => break,
            Ok(CommandOutcome::Continue) => {}
            Err(e) => eprintln!("{}", e),
        }
    }
    Ok(())
}
