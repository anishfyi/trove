use std::path::Path;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};

use crate::symbol::{Symbol, SymbolKind};

#[derive(Debug, Clone, Copy)]
pub enum SourceLanguage {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Bash,
}

impl SourceLanguage {
    pub fn detect(path: &Path) -> Option<Self> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("rs") => Some(Self::Rust),
            Some("py") => Some(Self::Python),
            Some("js" | "jsx" | "mjs" | "cjs") => Some(Self::JavaScript),
            Some("ts" | "tsx") => Some(Self::TypeScript),
            Some("go") => Some(Self::Go),
            Some("sh" | "bash") => Some(Self::Bash),
            _ => None,
        }
    }

    fn tree_sitter_language(self) -> Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::Bash => tree_sitter_bash::LANGUAGE.into(),
        }
    }

    fn query_source(self) -> &'static str {
        match self {
            Self::Rust => RUST_QUERY,
            Self::Python => PYTHON_QUERY,
            Self::JavaScript | Self::TypeScript => JS_QUERY,
            Self::Go => GO_QUERY,
            Self::Bash => BASH_QUERY,
        }
    }
}

pub struct ParsedFile {
    pub symbols: Vec<Symbol>,
    pub imports: Vec<String>,
}

pub fn parse_file(path: &Path, rel_path: &str, content: &str, hash: &str) -> Option<ParsedFile> {
    let lang = SourceLanguage::detect(path)?;
    let mut parser = Parser::new();
    parser.set_language(&lang.tree_sitter_language()).ok()?;
    let tree = parser.parse(content, None)?;

    let query = Query::new(&lang.tree_sitter_language(), lang.query_source()).ok()?;
    let mut cursor = QueryCursor::new();
    let root = tree.root_node();

    let mut symbols = Vec::new();
    let mut imports = Vec::new();

    let mut matches = cursor.matches(&query, root, content.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures {
            let node = capture.node;
            let capture_name = query.capture_names()[capture.index as usize];
            let text = node.utf8_text(content.as_bytes()).unwrap_or("").trim();
            if text.is_empty() {
                continue;
            }

            let start = node.start_position().row + 1;
            let end = node.end_position().row + 1;

            match capture_name {
                "import" => {
                    imports.push(text.to_string());
                }
                kind_name => {
                    if let Some(kind) = map_kind(kind_name) {
                        let name = extract_name(text, kind);
                        let id = Symbol::make_id(rel_path, &name, kind, start);
                        let signature = if kind == SymbolKind::Function
                            || kind == SymbolKind::Method
                            || kind == SymbolKind::Struct
                        {
                            Some(first_line(text))
                        } else {
                            None
                        };
                        symbols.push(Symbol {
                            id,
                            name,
                            kind,
                            path: rel_path.to_string(),
                            start_line: start,
                            end_line: end,
                            signature,
                            owner: None,
                            dependencies: Vec::new(),
                            references: Vec::new(),
                            complexity: estimate_complexity(content, start, end),
                            hash: hash.to_string(),
                        });
                    }
                }
            }
        }
    }

    Some(ParsedFile { symbols, imports })
}

fn map_kind(name: &str) -> Option<SymbolKind> {
    match name {
        "function" => Some(SymbolKind::Function),
        "class" => Some(SymbolKind::Class),
        "interface" => Some(SymbolKind::Interface),
        "enum" => Some(SymbolKind::Enum),
        "method" => Some(SymbolKind::Method),
        "struct" => Some(SymbolKind::Struct),
        "trait" => Some(SymbolKind::Trait),
        "module" => Some(SymbolKind::Module),
        "test" => Some(SymbolKind::Test),
        "command" => Some(SymbolKind::Command),
        "model" => Some(SymbolKind::Model),
        "migration" => Some(SymbolKind::Migration),
        "route" => Some(SymbolKind::Route),
        _ => None,
    }
}

fn extract_name(text: &str, kind: SymbolKind) -> String {
    let first = first_line(text);
    match kind {
        SymbolKind::Function | SymbolKind::Method => first
            .split('(')
            .next()
            .unwrap_or(&first)
            .split_whitespace()
            .last()
            .unwrap_or(&first)
            .to_string(),
        SymbolKind::Struct | SymbolKind::Class | SymbolKind::Enum | SymbolKind::Trait
        | SymbolKind::Interface | SymbolKind::Module => first
            .split_whitespace()
            .last()
            .unwrap_or(&first)
            .trim_matches(|c| c == '{' || c == ':' || c == '(')
            .to_string(),
        _ => first.chars().take(60).collect(),
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or(text).trim().to_string()
}

fn estimate_complexity(content: &str, start: usize, end: usize) -> usize {
    let lines: Vec<&str> = content.lines().collect();
    let slice = if start > 0 && start <= lines.len() {
        let e = end.min(lines.len());
        &lines[start.saturating_sub(1)..e]
    } else {
        &lines[..]
    };
    let joined = slice.join("\n");
    let branches = joined.matches("if ").count()
        + joined.matches("for ").count()
        + joined.matches("while ").count()
        + joined.matches("match ").count()
        + joined.matches("switch ").count();
    branches + slice.len() / 10
}

const RUST_QUERY: &str = r#"
(function_item name: (identifier) @name) @function
(struct_item name: (type_identifier) @name) @struct
(enum_item name: (type_identifier) @name) @enum
(trait_item name: (type_identifier) @name) @trait
(impl_item) @class
(mod_item name: (identifier) @name) @module
(use_declaration) @import
"#;

const PYTHON_QUERY: &str = r#"
(function_definition name: (identifier) @name) @function
(class_definition name: (identifier) @name) @class
(decorated_definition definition: (function_definition name: (identifier) @name)) @function
(decorated_definition definition: (class_definition name: (identifier) @name)) @class
(import_statement) @import
(import_from_statement) @import
"#;

const JS_QUERY: &str = r#"
(function_declaration name: (identifier) @name) @function
(class_declaration name: (identifier) @name) @class
(method_definition name: (property_identifier) @name) @method
(lexical_declaration (variable_declarator name: (identifier) @name value: (arrow_function))) @function
(import_statement) @import
"#;

const GO_QUERY: &str = r#"
(function_declaration name: (identifier) @name) @function
(method_declaration name: (field_identifier) @name) @method
(type_declaration (type_spec name: (type_identifier) @name)) @struct
(import_declaration) @import
"#;

const BASH_QUERY: &str = r#"
(function_definition name: (word) @name) @function
(command name: (command_name (word) @name)) @command
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parses_rust_fn() {
        let src = "pub fn hello() {}\n";
        let parsed = parse_file(Path::new("lib.rs"), "lib.rs", src, "abc").unwrap();
        assert!(!parsed.symbols.is_empty());
        assert_eq!(parsed.symbols[0].kind, SymbolKind::Function);
    }
}
