use crate::types::{parse_ptype_output, TypeLayout};
use regex::Regex;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const MAX_DUMP_BYTES: usize = 512;
const VAR_CREATE_AUTO: &str = "-";

#[derive(Debug, Clone, Copy)]
pub enum Endian {
    Little,
    Big,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct LocalVar {
    pub name: String,
    pub ty: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MemoryDump {
    pub expr: String,
    pub ty: Option<String>,
    pub address: String,
    pub bytes: Vec<u8>,
    pub word_size: usize,
    pub requested: usize,
    pub endian: Endian,
    pub arch: Option<String>,
    pub truncated_from: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct StoppedLocation {
    pub func: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub reason: Option<String>,
    pub arch: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BreakpointInfo {
    pub number: u32,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub func: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MiResponse {
    pub status: MiStatus,
    pub result: String,
    pub oob: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum MiStatus {
    Done,
    Running,
    Error(String),
    Other(String),
}

pub struct MiSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    verbose: bool,
    pub word_size: usize,
    word_known: bool,
    pub endian: Endian,
    pub arch: Option<String>,
}

impl MiSession {
    pub fn start(gdb_bin: &str, target: &str, args: &[String], verbose: bool) -> Result<Self> {
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
        Ok(())
    }

    /// Send a raw MI command (no added token) and collect the response until the prompt.
    pub fn exec_command(&mut self, cmd: &str) -> Result<MiResponse> {
        self.send_line(cmd)?;
        self.read_response()
    }

    /// Insert breakpoint at main, run, and wait until it stops.
    pub fn run_to_main(&mut self) -> Result<()> {
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
        let cmd = format!("-interpreter-exec console \"ptype {}\"", symbol);
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
                self.endian = parse_endian(&val);
                if matches!(self.endian, Endian::Unknown) && self.verbose {
                    eprintln!("[warn] could not parse endian from '{}'", val);
                }
                return;
            }
        }
        if self.verbose {
            eprintln!("[warn] failed to detect endian; leaving Unknown");
        }

        // Try to guess from arch if already known.
        if let Some(arch) = &self.arch {
            if let Some(guessed) = guess_endian_from_arch(arch) {
                self.endian = guessed;
                return;
            }
        }
        // Last resort: assume little-endian (common on modern targets).
        self.endian = Endian::Little;
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

fn parse_status(line: &str) -> MiStatus {
    if line.starts_with("^done") {
        MiStatus::Done
    } else if line.starts_with("^running") {
        MiStatus::Running
    } else if line.starts_with("^error") {
        let msg = parse_msg_field(line).unwrap_or_else(|| line.to_string());
        MiStatus::Error(msg)
    } else {
        MiStatus::Other(line.to_string())
    }
}

fn parse_msg_field(s: &str) -> Option<String> {
    Regex::new(r#"msg="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(s).map(|c| c[1].to_string()))
}

fn parse_value_field(s: &str) -> Option<String> {
    Regex::new(r#"value="((?:\\.|[^"])*)""#)
        .ok()
        .and_then(|re| re.captures(s).map(|c| unescape_value(&c[1])))
}

fn parse_type_field(s: &str) -> Option<String> {
    Regex::new(r#"type="((?:\\.|[^"])*)""#)
        .ok()
        .and_then(|re| re.captures(s).map(|c| unescape_value(&c[1])))
}

fn parse_addr_field(s: &str) -> Option<String> {
    Regex::new(r#"addr="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(s).map(|c| c[1].to_string()))
}

fn parse_memory_contents(s: &str) -> Result<Vec<u8>> {
    // Preferred MI form: memory=[{...,bytes="aabbcc"}]
    if let Some(caps) = Regex::new(r#"bytes="([0-9a-fA-F]+)""#)?.captures(s) {
        return hex_str_to_bytes(&caps[1]);
    }
    // Another form: contents="aa bb cc" or contents="aabbcc"
    if let Some(caps) = Regex::new(r#"contents="([^"]+)""#)?.captures(s) {
        let hex = &caps[1];
        if hex.contains(' ') {
            return Ok(split_hex_bytes(hex));
        } else {
            return hex_str_to_bytes(hex);
        }
    }
    // Common MI form: contents=["0xaa","0xbb",...]
    if let Some(caps) = Regex::new(r#"contents=\[([^\]]+)\]"#)?.captures(s) {
        return parse_hex_list(&caps[1]);
    }
    // Fallback for data=[...] form (legacy).
    if let Some(caps) = Regex::new(r#"data=\[([^\]]+)\]"#)?.captures(s) {
        return parse_hex_list(&caps[1]);
    }
    Err("no memory contents found".into())
}

fn parse_hex_list(list: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for part in list.split(',') {
        if let Some(b) = parse_hex_byte(part) {
            bytes.push(b);
        }
    }
    Ok(bytes)
}

fn split_hex_bytes(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for part in s.split_whitespace() {
        if let Some(b) = parse_hex_byte(part) {
            out.push(b);
        }
    }
    out
}

fn parse_locals(s: &str) -> Vec<LocalVar> {
    let mut locals = Vec::new();
    if let Ok(re) = Regex::new(
        r#"\{[^}]*name="(?P<name>[^"]+)"[^}]*?(?:type="(?P<type>(?:\\.|[^"])*)")?[^}]*?(?:value="(?P<value>(?:\\.|[^"])*)")?[^}]*\}"#,
    ) {
        for cap in re.captures_iter(s) {
            if let Some(name) = cap.name("name").map(|m| m.as_str().to_string()) {
                let value = cap.name("value").map(|m| unescape_value(m.as_str()));
                let ty = cap.name("type").map(|m| unescape_value(m.as_str()));
                locals.push(LocalVar { name, ty, value });
            }
        }
    }
    if locals.is_empty() {
        if let Ok(name_re) = Regex::new("name=\"([^\"]+)\"") {
            for cap in name_re.captures_iter(s) {
                if let Some(name) = cap.get(1).map(|m| m.as_str().to_string()) {
                    let value = parse_value_field(s);
                    locals.push(LocalVar {
                        name,
                        ty: None,
                        value,
                    });
                }
            }
        }
    }
    locals
}

fn parse_usize(s: &str) -> std::result::Result<usize, String> {
    let trimmed = s.trim();
    if let Some(hex) = trimmed.strip_prefix("0x") {
        usize::from_str_radix(hex, 16).map_err(|e| format!("parse hex usize '{}': {}", trimmed, e))
    } else {
        trimmed
            .parse::<usize>()
            .map_err(|e| format!("parse usize '{}': {}", trimmed, e))
    }
}

fn bytes_to_u64(bytes: &[u8], endian: Endian) -> u64 {
    let mut buf = [0u8; 8];
    let len = bytes.len().min(8);
    if matches!(endian, Endian::Big) {
        buf[8 - len..].copy_from_slice(&bytes[..len]);
        u64::from_be_bytes(buf)
    } else {
        buf[..len].copy_from_slice(&bytes[..len]);
        u64::from_le_bytes(buf)
    }
}

fn parse_hex_byte(raw: &str) -> Option<u8> {
    let trimmed = raw.trim().trim_matches('"');
    if trimmed.is_empty() {
        return None;
    }
    let num = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    u8::from_str_radix(num, 16).ok()
}

fn hex_str_to_bytes(s: &str) -> Result<Vec<u8>> {
    if s.len() % 2 != 0 {
        return Err("odd-length hex string in memory contents".into());
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let byte = &s[i..i + 2];
        let b = u8::from_str_radix(byte, 16)
            .map_err(|_| format!("invalid hex byte '{}' in memory contents", byte))?;
        out.push(b);
        i += 2;
    }
    Ok(out)
}

fn unescape_value(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.peek() {
                match *next {
                    '\\' => {
                        out.push('\\');
                        chars.next();
                        continue;
                    }
                    '"' => {
                        out.push('"');
                        chars.next();
                        continue;
                    }
                    'n' => {
                        out.push('\n');
                        chars.next();
                        continue;
                    }
                    't' => {
                        out.push('\t');
                        chars.next();
                        continue;
                    }
                    '0' => {
                        // Preserve explicit \0 / \000 sequences verbatim so downstream
                        // pretty-printers can decide how to show them.
                        out.push('\\');
                        out.push('0');
                        while let Some('0') = chars.peek() {
                            out.push('0');
                            chars.next();
                        }
                        continue;
                    }
                    _ => {}
                }
            }
        }
        out.push(c);
    }
    out
}

fn mi_escape(expr: &str) -> String {
    let mut out = String::with_capacity(expr.len() + 2);
    out.push('"');
    for ch in expr.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn parse_stopped(line: &str) -> StoppedLocation {
    let reason = Regex::new(r#"reason="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(line).map(|c| c[1].to_string()));
    let func = Regex::new(r#"func="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(line).map(|c| c[1].to_string()));
    let file = Regex::new(r#"file="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(line).map(|c| c[1].to_string()));
    let line_no = Regex::new(r#"line="([0-9]+)""#)
        .ok()
        .and_then(|re| re.captures(line).and_then(|c| c[1].parse::<u32>().ok()));
    let arch = Regex::new(r#"arch="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(line).map(|c| c[1].to_string()));
    StoppedLocation {
        func,
        file,
        line: line_no,
        reason,
        arch,
    }
}

fn parse_breakpoint(res: &str) -> BreakpointInfo {
    let num = Regex::new(r#"number="([0-9]+)""#)
        .ok()
        .and_then(|re| re.captures(res).and_then(|c| c[1].parse::<u32>().ok()))
        .unwrap_or(0);
    let func = Regex::new(r#"func="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(res).map(|c| c[1].to_string()));
    let file = Regex::new(r#"file="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(res).map(|c| c[1].to_string()));
    let line = Regex::new(r#"line="([0-9]+)""#)
        .ok()
        .and_then(|re| re.captures(res).and_then(|c| c[1].parse::<u32>().ok()));
    BreakpointInfo {
        number: num,
        file,
        line,
        func,
    }
}

fn parse_var_name(s: &str) -> Option<String> {
    Regex::new(r#"name="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(s).map(|c| c[1].to_string()))
}

fn parse_endian(val: &str) -> Endian {
    let lower = val.to_ascii_lowercase();
    if lower.contains("little") {
        Endian::Little
    } else if lower.contains("big") {
        Endian::Big
    } else {
        Endian::Unknown
    }
}

fn guess_endian_from_arch(arch: &str) -> Option<Endian> {
    let a = arch.to_ascii_lowercase();
    if a.contains("x86") || a.contains("amd64") || a.contains("i386") {
        return Some(Endian::Little);
    }
    if a.contains("aarch64") || a.contains("arm") {
        return Some(Endian::Little);
    }
    if a.contains("riscv") {
        return Some(Endian::Little);
    }
    None
}
