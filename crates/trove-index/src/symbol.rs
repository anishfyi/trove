use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Class,
    Interface,
    Enum,
    Method,
    Struct,
    Trait,
    Module,
    Route,
    Query,
    Migration,
    Model,
    Command,
    Test,
    Variable,
    Import,
    Other,
}

impl SymbolKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Enum => "enum",
            Self::Method => "method",
            Self::Struct => "struct",
            Self::Trait => "trait",
            Self::Module => "module",
            Self::Route => "route",
            Self::Query => "query",
            Self::Migration => "migration",
            Self::Model => "model",
            Self::Command => "command",
            Self::Test => "test",
            Self::Variable => "variable",
            Self::Import => "import",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub id: String,
    pub name: String,
    pub kind: SymbolKind,
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub signature: Option<String>,
    pub owner: Option<String>,
    pub dependencies: Vec<String>,
    pub references: Vec<String>,
    pub complexity: usize,
    pub hash: String,
}

impl Symbol {
    pub fn make_id(path: &str, name: &str, kind: SymbolKind, line: usize) -> String {
        format!("{}:{}:{}:{}", path, kind.label(), name, line)
    }
}
