pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

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

#[derive(Debug, Clone)]
pub struct GlobalVar {
    pub name: String,
    pub type_name: String,
    pub value: String,
    pub address: u64,
}
