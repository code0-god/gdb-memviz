use regex::Regex;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, Clone)]
pub struct LocalVar {
    pub name: String,
    pub ty: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MemoryDump {
    pub address: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct StoppedLocation {
    pub func: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub reason: Option<String>,
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
        })
    }

    /// Drain gdb banner until the initial prompt, echoing to stdout.
    pub fn drain_initial_output(&mut self) -> Result<()> {
        let lines = self.read_until_prompt(false)?;
        for line in lines {
            println!("{}", line);
        }
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
        }
        Ok(locals)
    }

    /// Evaluate address of a symbol using `-data-evaluate-expression`.
    pub fn evaluate_address(&mut self, symbol: &str) -> Result<String> {
        let cmd = format!("-data-evaluate-expression &{}", symbol);
        let resp = self.exec_command(&cmd)?;
        if let MiStatus::Error(msg) = resp.status.clone() {
            return Err(format!("{}", msg).into());
        }
        parse_value_field(&resp.result).ok_or_else(|| "address not found in MI response".into())
    }

    /// Evaluate arbitrary expression and return value string.
    pub fn evaluate_expression(&mut self, expr: &str) -> Result<String> {
        let cmd = format!("-data-evaluate-expression {}", expr);
        let resp = self.exec_command(&cmd)?;
        if let MiStatus::Error(msg) = resp.status.clone() {
            return Err(format!("{}", msg).into());
        }
        parse_value_field(&resp.result).ok_or_else(|| "value not found in MI response".into())
    }

    /// Read memory bytes from an address using `-data-read-memory-bytes`.
    pub fn read_memory(&mut self, address: &str, bytes: usize) -> Result<MemoryDump> {
        let cmd = format!("-data-read-memory-bytes {} {}", address, bytes);
        let resp = self.exec_command(&cmd)?;
        if let MiStatus::Error(msg) = resp.status.clone() {
            return Err(format!("{}", msg).into());
        }
        let raw = format!("{} {}", resp.result, resp.oob.join(" "));
        let addr = parse_addr_field(&raw).unwrap_or_else(|| address.to_string());
        let data = parse_memory_contents(&raw)?;
        Ok(MemoryDump {
            address: addr,
            bytes: data,
        })
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
                println!("{}", trimmed);
                break;
            }
            if trimmed.starts_with("^error") {
                return Err(format!("gdb error: {}", trimmed).into());
            }
            // Echo other out-of-band records to help debugging.
            println!("{}", trimmed);
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
                println!("{}", trimmed);
                return Ok(parse_stopped(&trimmed));
            }
            if trimmed.starts_with("^error") {
                return Err(format!("gdb error: {}", trimmed).into());
            }
            // Other async records, echo for visibility.
            println!("{}", trimmed);
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
        r#"\{[^}]*name="(?P<name>[^"]+)"[^}]*?(?:type="(?P<type>[^"]+)")?[^}]*?(?:value="(?P<value>(?:\\.|[^"])*)")?[^}]*\}"#,
    ) {
        for cap in re.captures_iter(s) {
            let name = cap.name("name").map(|m| m.as_str().to_string());
            if let Some(name) = name {
                let value = cap.name("value").map(|m| unescape_value(m.as_str()));
                let ty = cap.name("type").map(|m| m.as_str().to_string());
                locals.push(LocalVar { name, ty, value });
            }
        }
    }
    if locals.is_empty() {
        if let Ok(name_re) = Regex::new(r#"name="([^"]+)""#) {
            for cap in name_re.captures_iter(s) {
                let name = cap.get(1).map(|m| m.as_str().to_string());
                if let Some(name) = name {
                    let value = parse_value_field(s);
                    locals.push(LocalVar { name, ty: None, value });
                }
            }
        }
    }
    locals
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
                    _ => {}
                }
            }
        }
        out.push(c);
    }
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
    StoppedLocation {
        func,
        file,
        line: line_no,
        reason,
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
