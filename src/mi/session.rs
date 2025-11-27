use crate::mi::models::{
    BreakpointInfo, Endian, LocalVar, MemoryDump, MiResponse, MiStatus, Result, StoppedLocation,
};
use crate::mi::parser::{
    bytes_to_u64, guess_endian_from_arch, mi_escape, parse_addr_field, parse_breakpoint,
    parse_endian, parse_locals, parse_memory_contents, parse_status, parse_stopped,
    parse_type_field, parse_usize, parse_value_field, parse_var_name,
};
use crate::types::{parse_ptype_output, TypeLayout};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const MAX_DUMP_BYTES: usize = 512;
const VAR_CREATE_AUTO: &str = "-";

pub struct MiSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    verbose: bool, // when true, echo MI traffic to stderr for debugging
    pub word_size: usize,
    word_known: bool,
    pub endian: Endian,
    pub arch: Option<String>,
}

impl MiSession {
    pub fn start(gdb_bin: &str, target: &str, args: &[String], verbose: bool) -> Result<Self> {
        // Spawn gdb in MI mode (`-i=mi`) with quiet banner. Target args are passed as-is.
        let mut cmd = Command::new(gdb_bin);
        cmd.arg("-q").arg("-i=mi").arg("--args").arg(target);
        for a in args {
            cmd.arg(a);
        }
        let mut child = match cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    return Err(format!(
                        "gdb binary '{}' not found. Install gdb or pass --gdb <path>",
                        gdb_bin
                    )
                    .into());
                } else {
                    return Err(format!("failed to launch gdb '{}': {}", gdb_bin, e).into());
                }
            }
        };

        let stdin = child.stdin.take().ok_or("failed to open gdb stdin")?;
        let stdout = child.stdout.take().ok_or("failed to open gdb stdout")?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            verbose,
            word_size: 8,
            word_known: false,
            endian: Endian::Unknown,
            arch: None,
        })
    }

    /// Drain gdb banner until the initial prompt, echoing only when verbose.
    pub fn drain_initial_output(&mut self) -> Result<()> {
        let lines = self.read_until_prompt(false)?;
        if self.verbose {
            for line in lines {
                eprintln!("[mi<-] {}", line);
            }
        }
        self.ensure_endian();
        self.ensure_arch();
        Ok(())
    }

    /// Send a raw MI command (no added token) and collect the response until the prompt.
    pub fn exec_command(&mut self, cmd: &str) -> Result<MiResponse> {
        self.send_line(cmd)?;
        self.read_response()
    }

    /// Insert breakpoint at main, run, and wait until it stops.
    pub fn run_to_main(&mut self) -> Result<()> {
        // Best-effort: set a breakpoint on main, run, and block until a stop event arrives.
        let resp = self.exec_command("-break-insert main")?;
        match resp.status {
            MiStatus::Error(msg) => {
                return Err(format!("failed to set breakpoint: {}", msg).into());
            }
            _ => {}
        }

        let resp = self.exec_command("-exec-run")?;
        if let MiStatus::Error(msg) = resp.status {
            return Err(format!("failed to run: {}", msg).into());
        }
        if !resp.oob.iter().any(|l| l.starts_with("*stopped")) {
            self.wait_for_stop()?;
        }
        Ok(())
    }

    /// Read current frame locals using `-stack-list-locals 2` (includes values).
    pub fn list_locals(&mut self) -> Result<Vec<LocalVar>> {
        let resp = self.exec_command("-stack-list-locals 2")?;
        if let MiStatus::Error(msg) = resp.status.clone() {
            return Err(format!("gdb error: {}", msg).into());
        }
        let raw = format!("{} {}", resp.result, resp.oob.join(" "));
        let mut locals = parse_locals(&raw);
        // Fallback: for locals without value, try evaluating directly.
        for var in locals.iter_mut() {
            if var.value.is_none() {
                if let Ok(val) = self.evaluate_expression(&var.name) {
                    var.value = Some(val);
                }
            }
            if var.ty.is_none() {
                if let Some(ty) = self.fetch_type(&var.name) {
                    var.ty = Some(ty);
                }
            }
        }
        Ok(locals)
    }

    #[allow(dead_code)]
    /// Evaluate address of a symbol using `-data-evaluate-expression`.
    pub fn evaluate_address(&mut self, symbol: &str) -> Result<String> {
        let expr = format!("&{}", symbol);
        let cmd = format!("-data-evaluate-expression {}", mi_escape(&expr));
        let resp = self.exec_command(&cmd)?;
        if let MiStatus::Error(msg) = resp.status.clone() {
            return Err(format!("{}", msg).into());
        }
        parse_value_field(&resp.result).ok_or_else(|| "address not found in MI response".into())
    }

    /// Evaluate arbitrary expression and return value string.
    pub fn evaluate_expression(&mut self, expr: &str) -> Result<String> {
        let cmd = format!("-data-evaluate-expression {}", mi_escape(expr));
        let resp = self.exec_command(&cmd)?;
        if let MiStatus::Error(msg) = resp.status.clone() {
            return Err(format!("{}", msg).into());
        }
        parse_value_field(&resp.result).ok_or_else(|| "value not found in MI response".into())
    }

    /// Run ptype and return console text.
    pub fn ptype_text(&mut self, symbol: &str) -> Result<String> {
        // We call into the CLI `ptype` because MI lacks a clean equivalent for pretty layout.
        let cmd = format!("-interpreter-exec console \"ptype /o {}\"", symbol);
        let resp = self.exec_command(&cmd)?;
        if let MiStatus::Error(msg) = resp.status.clone() {
            return Err(format!("{}", msg).into());
        }
        let mut out = String::new();
        for line in &resp.oob {
            if let Some(stripped) = line.strip_prefix("~\"") {
                let mut s = stripped.trim_end_matches('"').to_string();
                s = s.replace("\\n", "\n");
                out.push_str(&s);
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
        }
        if out.is_empty() {
            out.push_str(&resp.result);
        }
        Ok(out)
    }

    /// Fetch a parsed type layout using ptype; fall back to scalar.
    pub fn fetch_layout(&mut self, symbol: &str, size: usize) -> Option<TypeLayout> {
        if let Ok(txt) = self.ptype_text(symbol) {
            return Some(parse_ptype_output(&txt, self.word_size, size));
        }
        None
    }

    /// Fetch a parsed type layout for an arbitrary type name (e.g., "struct Node").
    pub fn fetch_layout_for_type(&mut self, type_name: &str) -> Option<TypeLayout> {
        let size = self.evaluate_sizeof(type_name).unwrap_or(self.word_size);
        if let Ok(txt) = self.ptype_text(type_name) {
            return Some(parse_ptype_output(&txt, self.word_size, size));
        }
        None
    }

    /// Evaluate sizeof(<expr>) and return bytes.
    pub fn evaluate_sizeof(&mut self, expr: &str) -> Result<usize> {
        let expr = format!("sizeof({})", expr);
        let cmd = format!("-data-evaluate-expression {}", mi_escape(&expr));
        let resp = self.exec_command(&cmd)?;
        if let MiStatus::Error(msg) = resp.status.clone() {
            return Err(format!("{}", msg).into());
        }
        let raw = parse_value_field(&resp.result).ok_or("sizeof returned no value")?;
        parse_usize(&raw).map_err(|e| e.into())
    }

    /// Ensure word size is detected (sizeof(void*)), defaulting to 8 on failure.
    pub fn ensure_word_size(&mut self) {
        if self.word_known {
            return;
        }
        match self.evaluate_sizeof("void*") {
            Ok(sz) if sz > 0 => {
                self.word_size = sz;
                self.word_known = true;
            }
            _ => {
                // If gdb cannot answer, assume 64-bit to keep dumps aligned.
                if self.verbose {
                    eprintln!("[warn] failed to detect word size; defaulting to 8");
                }
                self.word_size = 8;
                self.word_known = true;
            }
        }
    }

    /// Detect endian via `-gdb-show endian` (best-effort).
    pub fn ensure_endian(&mut self) {
        if !matches!(self.endian, Endian::Unknown) {
            return;
        }
        let resp = self.exec_command("-gdb-show endian");
        if let Ok(r) = resp {
            if let Some(val) = parse_value_field(&r.result) {
                let parsed = parse_endian(&val);
                if !matches!(parsed, Endian::Unknown) {
                    self.endian = parsed;
                    return;
                } else if self.verbose {
                    eprintln!("[warn] could not parse endian from '{}'", val);
                }
            }
        } else if self.verbose {
            eprintln!("[warn] failed to detect endian; leaving Unknown");
        }

        // Try to guess from arch if already known; otherwise default to little.
        if let Some(arch) = &self.arch {
            if let Some(guessed) = guess_endian_from_arch(arch) {
                self.endian = guessed;
                return;
            }
        }
        // Last resort: assume little-endian (common on modern targets).
        self.endian = Endian::Little;
    }

    /// Detect architecture via `-gdb-show architecture` (best-effort).
    pub fn ensure_arch(&mut self) {
        if self.arch.is_some() {
            return;
        }
        if let Ok(resp) = self.exec_command("-gdb-show architecture") {
            if let Some(val) = parse_value_field(&resp.result) {
                let trimmed = val.trim();
                if !trimmed.is_empty() && trimmed != "auto" {
                    self.arch = Some(trimmed.to_string());
                    return;
                }
            }
        }
        // Leave as None if gdb cannot provide it; later stop events may fill it.
    }

    /// Try to obtain the inferior process pid from `info proc`.
    pub fn inferior_pid(&mut self) -> Result<u32> {
        let cmd = "-interpreter-exec console \"info proc\"";
        let resp = self.exec_command(cmd)?;
        let mut text = String::new();
        text.push_str(&resp.result);
        text.push('\n');
        for line in &resp.oob {
            let clean = line
                .trim_start_matches("~\"")
                .trim_end_matches('"')
                .replace("\\n", "\n");
            text.push_str(&clean);
            text.push('\n');
        }
        for line in text.lines() {
            if line.contains("process") {
                let mut parts = line.split_whitespace();
                while let Some(tok) = parts.next() {
                    if tok == "process" {
                        if let Some(pid_str) = parts.next() {
                            if let Ok(pid) = pid_str.parse::<u32>() {
                                return Ok(pid);
                            }
                        }
                    }
                }
            }
        }
        Err("could not determine inferior pid from 'info proc'".into())
    }

    /// Evaluate expression and return (type, value) strings.
    pub fn eval_expr_type_and_value(&mut self, expr: &str) -> Result<(String, String)> {
        let cmd = format!("-data-evaluate-expression {}", mi_escape(expr));
        let resp = self.exec_command(&cmd)?;
        if let MiStatus::Error(msg) = resp.status.clone() {
            return Err(format!("{}", msg).into());
        }
        let value = parse_value_field(&resp.result)
            .or_else(|| resp.oob.iter().find_map(|l| parse_value_field(l)))
            .ok_or("value not found in MI response")?;

        let ty = parse_type_field(&resp.result)
            .or_else(|| self.fetch_type(expr))
            .unwrap_or_else(|| "unknown".to_string());
        Ok((ty, value))
    }

    /// Evaluate address of an expression and return as u64.
    pub fn eval_address_of_expr(&mut self, expr: &str) -> Result<u64> {
        let addr_expr = format!("&({})", expr);
        let cmd = format!("-data-evaluate-expression {}", mi_escape(&addr_expr));
        let resp = self.exec_command(&cmd)?;
        if let MiStatus::Error(msg) = resp.status.clone() {
            return Err(format!("{}", msg).into());
        }
        let raw = parse_value_field(&resp.result).ok_or("address not found in MI response")?;
        let val = parse_usize(&raw)?;
        Ok(val as u64)
    }

    /// Higher-level memory dump that respects sizeof(expr) and word size.
    pub fn memory_dump(&mut self, expr: &str, override_len: Option<usize>) -> Result<MemoryDump> {
        self.ensure_word_size();
        self.ensure_endian();

        let addr_expr = format!("&{}", expr);
        let addr = self.evaluate_expression(&addr_expr)?;

        let mut requested = match override_len {
            Some(len) => len,
            None => self.evaluate_sizeof(expr).unwrap_or(32),
        };
        if requested == 0 {
            requested = 32;
        }
        // Cap dump size to avoid overwhelming output/logs.
        let mut truncated_from = None;
        if requested > MAX_DUMP_BYTES {
            truncated_from = Some(requested);
            requested = MAX_DUMP_BYTES;
        }
        let (addr, bytes) = self.read_memory_bytes(&addr, requested)?;
        // If endian is still unknown, use arch hint or default little.
        if matches!(self.endian, Endian::Unknown) {
            if let Some(arch) = &self.arch {
                if let Some(e) = guess_endian_from_arch(arch) {
                    self.endian = e;
                } else {
                    self.endian = Endian::Little;
                }
            } else {
                self.endian = Endian::Little;
            }
        }
        Ok(MemoryDump {
            expr: expr.to_string(),
            ty: self.fetch_type(expr),
            address: addr,
            bytes,
            word_size: self.word_size,
            requested,
            endian: self.endian,
            arch: self.arch.clone(),
            truncated_from,
        })
    }

    /// Read a pointer-sized value at the given address, honoring struct field size overrides.
    pub fn read_pointer_at(&mut self, address: u64, size_override: Option<usize>) -> Result<u64> {
        self.ensure_word_size();
        self.ensure_endian();
        let size = size_override.unwrap_or(self.word_size).max(1);
        let (_, bytes) = self.read_memory_bytes(&format!("0x{:x}", address), size)?;
        Ok(bytes_to_u64(&bytes, self.endian))
    }

    /// Fetch type name using -var-create/-var-delete. Returns None on failure.
    fn fetch_type(&mut self, expr: &str) -> Option<String> {
        let cmd = format!("-var-create {} * {}", VAR_CREATE_AUTO, expr);
        let resp = self.exec_command(&cmd).ok()?;
        if let MiStatus::Error(_) = resp.status {
            return None;
        }
        let name = parse_var_name(&resp.result)?;
        let ty = parse_type_field(&resp.result);
        let _ = self.exec_command(&format!("-var-delete {}", name));
        ty
    }

    /// Read memory bytes from an address using `-data-read-memory-bytes`.
    fn read_memory_bytes(&mut self, address: &str, bytes: usize) -> Result<(String, Vec<u8>)> {
        let cmd = format!("-data-read-memory-bytes {} {}", address, bytes);
        let resp = self.exec_command(&cmd)?;
        if let MiStatus::Error(msg) = resp.status.clone() {
            return Err(format!("{}", msg).into());
        }
        let raw = format!("{} {}", resp.result, resp.oob.join(" "));
        let addr = parse_addr_field(&raw).unwrap_or_else(|| address.to_string());
        let data = parse_memory_contents(&raw)?;
        Ok((addr, data))
    }

    /// Wait for a `*stopped` event. Used after run when the initial response did not include it.
    pub fn wait_for_stop(&mut self) -> Result<()> {
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line)?;
            if n == 0 {
                return Err("gdb exited unexpectedly".into());
            }
            let trimmed = line.trim().to_string();
            if trimmed.is_empty() || trimmed == "(gdb)" {
                continue;
            }
            if self.verbose {
                eprintln!("[mi<-] {}", trimmed);
            }
            if trimmed.starts_with("*stopped") {
                let loc = parse_stopped(&trimmed);
                if self.arch.is_none() {
                    self.arch = loc.arch.clone();
                }
                break;
            }
            if trimmed.starts_with("^error") {
                return Err(format!("gdb error: {}", trimmed).into());
            }
            // Echo other out-of-band records to help debugging.
            if self.verbose {
                eprintln!("[mi<-] {}", trimmed);
            }
        }
        Ok(())
    }

    /// Continue execution until next stop.
    pub fn exec_continue(&mut self) -> Result<StoppedLocation> {
        let resp = self.exec_command("-exec-continue")?;
        if let MiStatus::Error(msg) = resp.status {
            return Err(format!("continue failed: {}", msg).into());
        }
        let stop = self.wait_for_stop_capture()?;
        Ok(stop)
    }

    /// Step over.
    pub fn exec_next(&mut self) -> Result<StoppedLocation> {
        let resp = self.exec_command("-exec-next")?;
        if let MiStatus::Error(msg) = resp.status {
            return Err(format!("next failed: {}", msg).into());
        }
        let stop = self.wait_for_stop_capture()?;
        Ok(stop)
    }

    /// Step into.
    pub fn exec_step(&mut self) -> Result<StoppedLocation> {
        let resp = self.exec_command("-exec-step")?;
        if let MiStatus::Error(msg) = resp.status {
            return Err(format!("step failed: {}", msg).into());
        }
        let stop = self.wait_for_stop_capture()?;
        Ok(stop)
    }

    /// Insert a breakpoint at given location string.
    pub fn break_insert(&mut self, location: &str) -> Result<BreakpointInfo> {
        let cmd = format!("-break-insert {}", location);
        let resp = self.exec_command(&cmd)?;
        if let MiStatus::Error(msg) = resp.status {
            return Err(format!("break insert failed: {}", msg).into());
        }
        Ok(parse_breakpoint(&resp.result))
    }

    /// Wait for stopped and parse the location.
    fn wait_for_stop_capture(&mut self) -> Result<StoppedLocation> {
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line)?;
            if n == 0 {
                return Err("gdb exited unexpectedly".into());
            }
            let trimmed = line.trim().to_string();
            if trimmed.is_empty() || trimmed == "(gdb)" {
                continue;
            }
            if self.verbose {
                eprintln!("[mi<-] {}", trimmed);
            }
            if trimmed.starts_with("*stopped") {
                let loc = parse_stopped(&trimmed);
                if self.arch.is_none() {
                    self.arch = loc.arch.clone();
                }
                return Ok(loc);
            }
            if trimmed.starts_with("^error") {
                return Err(format!("gdb error: {}", trimmed).into());
            }
            // Other async records, echo for visibility.
            if self.verbose {
                eprintln!("[mi<-] {}", trimmed);
            }
        }
    }

    /// Attempt to shut down gdb cleanly.
    pub fn shutdown(&mut self) {
        let _ = self.send_line("-gdb-exit");
        let _ = self.child.wait();
    }

    fn send_line(&mut self, cmd: &str) -> Result<()> {
        let mut line = cmd.to_string();
        line.push('\n');
        if self.verbose {
            eprintln!("[mi->] {}", cmd);
        }
        self.stdin.write_all(line.as_bytes())?;
        self.stdin.flush()?;
        Ok(())
    }

    fn read_response(&mut self) -> Result<MiResponse> {
        // Collect a single result record (^done/^error/...) and any preceding async output.
        let mut oob = Vec::new();
        let mut result_line: Option<String> = None;
        let mut saw_prompt = false;
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line)?;
            if n == 0 {
                return Err("gdb exited unexpectedly".into());
            }
            let trimmed = line.trim().to_string();
            if trimmed.is_empty() {
                continue;
            }
            if self.verbose {
                eprintln!("[mi<-] {}", trimmed);
            }
            if trimmed == "(gdb)" {
                saw_prompt = true;
                if result_line.is_some() {
                    break;
                } else {
                    continue;
                }
            }
            if trimmed.starts_with('^') {
                result_line = Some(trimmed.clone());
                if saw_prompt {
                    break;
                } else {
                    continue;
                }
            }
            oob.push(trimmed);
        }
        let res = result_line.unwrap_or_else(|| String::from("^error,msg=\"missing result\""));
        let status = parse_status(&res);
        Ok(MiResponse {
            status,
            result: res,
            oob,
        })
    }

    fn read_until_prompt(&mut self, require_result: bool) -> Result<Vec<String>> {
        // Helper for initial banner drain; returns all lines until a prompt, optionally
        // insisting that we saw a result record before exiting.
        let mut lines = Vec::new();
        let mut saw_result = false;
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line)?;
            if n == 0 {
                return Err("gdb exited unexpectedly".into());
            }
            let trimmed = line.trim().to_string();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed == "(gdb)" {
                if !require_result || saw_result {
                    break;
                }
                continue;
            }
            if trimmed.starts_with('^') {
                saw_result = true;
            }
            lines.push(trimmed);
        }
        Ok(lines)
    }
}
