use crate::logger::log_debug;
use crate::mi::models::{
    BreakpointInfo, Endian, GlobalVar, LocalVar, MemoryDump, MiResponse, MiStatus, Result,
    StoppedLocation,
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

#[derive(Debug)]
pub struct MiSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    verbose: bool, // when true, echo MI traffic to stderr for debugging
    pub word_size: usize,
    word_known: bool,
    pub endian: Endian,
    pub arch: Option<String>,
    target_hint: String,
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
            target_hint: std::path::Path::new(target)
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
                .unwrap_or_default(),
        })
    }

    /// Drain gdb banner until the initial prompt, echoing only when verbose.
    pub fn drain_initial_output(&mut self) -> Result<()> {
        let lines = self.read_until_prompt(false)?;
        if self.verbose {
            for line in lines {
                log_debug(&format!("[mi<-] {}", line));
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

    /// Insert breakpoint at main, run, and wait until it stops. Returns the stop location.
    pub fn run_to_main(&mut self) -> Result<StoppedLocation> {
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
        if let Some(line) = resp.oob.iter().find(|l| l.starts_with("*stopped")) {
            return Ok(parse_stopped(line));
        }
        let stop = self.wait_for_stop_capture()?;
        Ok(stop)
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
                    log_debug("[warn] failed to detect word size; defaulting to 8");
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
                    log_debug(&format!("[warn] could not parse endian from '{}'", val));
                }
            }
        } else if self.verbose {
            log_debug("[warn] failed to detect endian; leaving Unknown");
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
        let resp = self.exec_command(&cmd)?;
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

    /// Get the current frame's source file (fullname or file) if available.
    pub fn current_frame_file(&mut self) -> Option<String> {
        let resp = self.exec_command("-stack-info-frame").ok()?;
        parse_field(&resp.result, "fullname").or_else(|| parse_field(&resp.result, "file"))
    }

    /// List global variables visible to gdb (console-based parsing).
    /// When `filter_file` is Some("foo.c"), only variables under that File block are returned.
    pub fn list_globals(&mut self, filter_file: Option<&str>) -> Result<Vec<GlobalVar>> {
        let resp = self.exec_command("-interpreter-exec console \"info variables\"")?;
        let mut text = String::new();
        text.push_str(&resp.result.replace("\\n", "\n").replace("\\t", "\t"));
        text.push('\n');
        for line in &resp.oob {
            let cleaned = line
                .trim_start_matches("~\"")
                .trim_end_matches('"')
                .replace("\\n", "\n")
                .replace("\\t", "\t");
            text.push_str(&cleaned);
            text.push('\n');
        }

        Ok(parse_info_variables_output(&text, filter_file, self))
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

    /// Evaluate arbitrary expression and interpret the result as u64.
    pub fn eval_expr_u64(&mut self, expr: &str) -> Result<u64> {
        let cmd = format!("-data-evaluate-expression {}", mi_escape(expr));
        let resp = self.exec_command(&cmd)?;
        if let MiStatus::Error(msg) = resp.status.clone() {
            return Err(format!("{}", msg).into());
        }
        let raw = parse_value_field(&resp.result).ok_or("value field not found in MI response")?;
        // Try to scrape an address or number from the value field first.
        if let Some(addr) = parse_address_str(&raw) {
            return Ok(addr);
        }
        if let Some(first) = raw.split_whitespace().next() {
            if let Some(addr) = parse_address_str(first) {
                return Ok(addr);
            }
            if let Ok(v) = parse_usize(first) {
                return Ok(v as u64);
            }
        }
        // As a fallback, inspect the entire MI result.
        if let Some(addr) = parse_address_str(&resp.result) {
            return Ok(addr);
        }
        let val = parse_usize(&raw)?;
        Ok(val as u64)
    }

    /// Evaluate address of an expression and return as u64.
    pub fn eval_address_of_expr(&mut self, expr: &str) -> Result<u64> {
        let addr_expr = format!("&({})", expr);
        self.eval_expr_u64(&addr_expr)
    }

    /// Higher-level memory dump that respects sizeof(expr) and word size.
    pub fn memory_dump(&mut self, expr: &str, override_len: Option<usize>) -> Result<MemoryDump> {
        self.ensure_word_size();
        self.ensure_endian();

        let addr_u64 = self.eval_address_of_expr(expr)?;
        let addr_str = format!("0x{:x}", addr_u64);

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
        let (addr, bytes) = self.read_memory_bytes(&addr_str, requested)?;
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
        let mut saw_stop = false;
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
                log_debug(&format!("[mi<-] {}", trimmed));
            }
            if trimmed == "(gdb)" {
                if saw_stop {
                    return Ok(());
                }
                continue;
            }
            if trimmed.starts_with("*stopped") {
                let loc = parse_stopped(&trimmed);
                if self.arch.is_none() {
                    self.arch = loc.arch.clone();
                }
                saw_stop = true;
                continue;
            }
            if trimmed.starts_with("^error") {
                return Err(format!("gdb error: {}", trimmed).into());
            }
        }
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
        let mut stop: Option<StoppedLocation> = None;
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
                log_debug(&format!("[mi<-] {}", trimmed));
            }
            if trimmed == "(gdb)" {
                if let Some(loc) = stop {
                    return Ok(loc);
                } else {
                    continue;
                }
            }
            if trimmed.starts_with("*stopped") {
                let loc = parse_stopped(&trimmed);
                if self.arch.is_none() {
                    self.arch = loc.arch.clone();
                }
                stop = Some(loc);
                continue;
            }
            if trimmed.starts_with("^error") {
                return Err(format!("gdb error: {}", trimmed).into());
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
            log_debug(&format!("[mi->] {}", cmd));
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
                log_debug(&format!("[mi<-] {}", trimmed));
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

fn parse_global_decl(line: &str) -> Option<(String, String)> {
    // Examples:
    // "13:\tint g_counter;"
    // "char g_message[16];"
    let mut cleaned = line.trim();
    // Drop leading "<digits>:" prefix if present.
    if let Some(colon_idx) = cleaned.find(':') {
        if cleaned[..colon_idx].chars().all(|c| c.is_ascii_digit()) {
            cleaned = cleaned[colon_idx + 1..].trim();
        }
    }
    cleaned = cleaned.trim_end_matches(';').trim();
    let parts: Vec<&str> = cleaned.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let mut name = parts.last()?.trim().to_string();
    if let Some(idx) = name.find('[') {
        name = name[..idx].trim().to_string();
    }
    while name.starts_with('*') {
        name.remove(0);
    }
    let type_name = parts[..parts.len() - 1].join(" ");
    if type_name.is_empty() || name.is_empty() {
        return None;
    }
    Some((type_name, name))
}

fn parse_address_str(s: &str) -> Option<u64> {
    let trimmed = s.trim();
    if let Some(hex) = trimmed.strip_prefix("0x") {
        return u64::from_str_radix(hex, 16).ok();
    }
    if let Some(idx) = trimmed.find("0x") {
        let rest = &trimmed[idx + 2..];
        let hex_part: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        if !hex_part.is_empty() {
            return u64::from_str_radix(&hex_part, 16).ok();
        }
    }
    if let Ok(re) = regex::Regex::new(r"0x[0-9a-fA-F]+") {
        if let Some(mat) = re.find(trimmed) {
            let hex = mat.as_str().trim_start_matches("0x");
            return u64::from_str_radix(hex, 16).ok();
        }
    }
    None
}

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

fn parse_info_variables_output(
    output: &str,
    filter_file: Option<&str>,
    session: &mut MiSession,
) -> Vec<GlobalVar> {
    use std::path::Path;

    let filter_basename = filter_file.map(|s| s.to_string());
    let mut current_file: Option<String> = None;
    let mut globals = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("Non-debugging symbols") {
            break;
        }
        if trimmed.starts_with("All defined variables") {
            continue;
        }
        if trimmed.starts_with("File ") || trimmed.ends_with(':') {
            let header = trimmed
                .trim_start_matches("File ")
                .trim_end_matches(':')
                .trim()
                .to_string();
            current_file = Some(header);
            continue;
        }

        // Filter by current file basename if requested.
        if let Some(ref filter) = filter_basename {
            let Some(ref cur) = current_file else {
                continue;
            };
            let cur_base = Path::new(cur)
                .file_name()
                .and_then(|os| os.to_str())
                .unwrap_or(cur);
            if cur_base != filter {
                continue;
            }
        }

        if !trimmed.contains(';') || trimmed.contains('(') {
            continue;
        }
        if let Some((type_name, name)) = parse_global_decl(trimmed) {
            let val = session
                .evaluate_expression(&name)
                .unwrap_or_else(|_| "<unavailable>".to_string());
            let addr = session.eval_address_of_expr(&name).unwrap_or(0);
            globals.push(GlobalVar {
                name: name.to_string(),
                type_name: type_name.to_string(),
                value: val,
                address: addr,
            });
        }
    }
    globals
}
