use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct GlobalVarInfo {
    pub name: String,
    pub type_name: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub is_static: bool,
    pub is_function_scope: bool,
}

#[derive(Debug, Default, Clone)]
pub struct SymbolIndex {
    /// file basename -> globals defined in that file
    pub globals_by_file: HashMap<String, Vec<GlobalVarInfo>>,
}

#[derive(Debug, Clone, Copy)]
pub enum SymbolIndexMode {
    /// 전역 인덱스를 만들지 않음 (locals만 사용)
    None,
    /// 디버그 심볼만 사용 (기본 모드)
    DebugOnly,
    /// 디버그 + non-debug 심볼 모두 사용 (느리지만 완전)
    DebugAndNonDebug,
}

impl Default for SymbolIndexMode {
    fn default() -> Self {
        SymbolIndexMode::DebugOnly
    }
}

#[derive(Debug, Clone)]
pub struct GlobalVarWithValue {
    pub info: GlobalVarInfo,
    pub value: String,
    pub address: u64,
}
