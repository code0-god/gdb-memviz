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

#[derive(Debug, Clone)]
pub struct GlobalVarWithValue {
    pub info: GlobalVarInfo,
    pub value: String,
    pub address: u64,
}
