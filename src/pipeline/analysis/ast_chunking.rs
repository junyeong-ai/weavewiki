use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::analyzer::parser::language::Language;

/// When an impl/class block exceeds this many lines, split at method boundaries.
const MAX_BLOCK_LINES_BEFORE_SPLIT: u32 = 80;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AstBoundary {
    pub start_line: u32,
    pub end_line: u32,
    pub kind: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExportedSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Class,
    Impl,
    Type,
    Const,
    Module,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Function => write!(f, "fn"),
            Self::Struct => write!(f, "struct"),
            Self::Enum => write!(f, "enum"),
            Self::Trait => write!(f, "trait"),
            Self::Class => write!(f, "class"),
            Self::Impl => write!(f, "impl"),
            Self::Type => write!(f, "type"),
            Self::Const => write!(f, "const"),
            Self::Module => write!(f, "mod"),
        }
    }
}

pub struct FileAstInfo {
    pub boundaries: Vec<AstBoundary>,
    pub exported_symbols: Vec<ExportedSymbol>,
}

fn get_ts_language(lang: Language) -> Option<tree_sitter::Language> {
    match lang {
        Language::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
        Language::Python => Some(tree_sitter_python::LANGUAGE.into()),
        Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx => {
            Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        }
        Language::Go => Some(tree_sitter_go::LANGUAGE.into()),
        _ => None,
    }
}

pub fn analyze_file_ast(path: &Path, content: &str) -> FileAstInfo {
    let lang = Language::from_path(path);
    let ts_lang = match get_ts_language(lang) {
        Some(l) => l,
        None => {
            return FileAstInfo {
                boundaries: Vec::new(),
                exported_symbols: Vec::new(),
            };
        }
    };

    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&ts_lang).is_err() {
        return FileAstInfo {
            boundaries: Vec::new(),
            exported_symbols: Vec::new(),
        };
    }

    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => {
            return FileAstInfo {
                boundaries: Vec::new(),
                exported_symbols: Vec::new(),
            };
        }
    };

    let root = tree.root_node();
    let mut boundaries = Vec::new();
    let mut exported_symbols = Vec::new();

    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        let kind = child.kind();

        let (is_boundary, symbol_kind) = match kind {
            "function_item" | "function_definition" | "function_declaration" => {
                (true, Some(SymbolKind::Function))
            }
            "impl_item" => (true, Some(SymbolKind::Impl)),
            "struct_item" => (true, Some(SymbolKind::Struct)),
            "enum_item" => (true, Some(SymbolKind::Enum)),
            "trait_item" => (true, Some(SymbolKind::Trait)),
            "class_definition" => (true, Some(SymbolKind::Class)),
            "method_definition" => (true, Some(SymbolKind::Function)),
            "type_declaration" => (true, Some(SymbolKind::Type)),
            "const_item" | "static_item" => (true, Some(SymbolKind::Const)),
            "mod_item" => (true, Some(SymbolKind::Module)),
            "decorated_definition" => (true, Some(SymbolKind::Function)),
            _ => (false, None),
        };

        if !is_boundary {
            continue;
        }

        let name = child
            .child_by_field_name("name")
            .or_else(|| child.child_by_field_name("type"))
            .and_then(|n| n.utf8_text(content.as_bytes()).ok())
            .unwrap_or("anonymous")
            .to_string();

        let start_line = child.start_position().row as u32 + 1;
        let end_line = child.end_position().row as u32 + 1;

        boundaries.push(AstBoundary {
            start_line,
            end_line,
            kind: kind.to_string(),
            name: name.clone(),
        });

        if let Some(sk) = symbol_kind {
            exported_symbols.push(ExportedSymbol {
                name: name.clone(),
                kind: sk,
                start_line,
                end_line,
            });
        }

        // For large impl/class blocks, add method-level boundaries
        if matches!(kind, "impl_item" | "class_definition")
            && end_line - start_line >= MAX_BLOCK_LINES_BEFORE_SPLIT
        {
            collect_method_boundaries(child, content, &name, &mut boundaries, &mut exported_symbols);
        }
    }

    FileAstInfo {
        boundaries,
        exported_symbols,
    }
}

fn collect_method_boundaries(
    parent: tree_sitter::Node<'_>,
    content: &str,
    parent_name: &str,
    boundaries: &mut Vec<AstBoundary>,
    exported_symbols: &mut Vec<ExportedSymbol>,
) {
    let body = parent.child_by_field_name("body")
        .or_else(|| {
            // Rust impl_item uses a direct block child rather than a named "body" field
            let mut cursor = parent.walk();
            parent.children(&mut cursor)
                .find(|c| c.kind() == "declaration_list" || c.kind() == "block")
        });

    let body = match body {
        Some(b) => b,
        None => return,
    };

    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        let is_method = matches!(
            child.kind(),
            "function_item" | "function_definition" | "method_definition" | "decorated_definition"
        );
        if !is_method {
            continue;
        }

        let method_name = child
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(content.as_bytes()).ok())
            .unwrap_or("anonymous");

        let start_line = child.start_position().row as u32 + 1;
        let end_line = child.end_position().row as u32 + 1;
        let qualified_name = format!("{parent_name}::{method_name}");

        boundaries.push(AstBoundary {
            start_line,
            end_line,
            kind: "method".to_string(),
            name: qualified_name.clone(),
        });

        exported_symbols.push(ExportedSymbol {
            name: qualified_name,
            kind: SymbolKind::Function,
            start_line,
            end_line,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_rust_file() {
        let content = r#"
pub fn hello() {
    println!("hello");
}

pub struct Foo {
    bar: i32,
}

impl Foo {
    pub fn new() -> Self {
        Foo { bar: 0 }
    }
}
"#;
        let info = analyze_file_ast(Path::new("test.rs"), content);
        assert!(info.boundaries.len() >= 3);
        assert!(info.exported_symbols.len() >= 3);

        let names: Vec<&str> = info.exported_symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"hello"));
        assert!(names.contains(&"Foo"));
    }

    #[test]
    fn test_analyze_python_file() {
        let content = r#"
def greet(name):
    print(f"Hello {name}")

class Greeter:
    def __init__(self):
        pass
"#;
        let info = analyze_file_ast(Path::new("test.py"), content);
        assert!(!info.boundaries.is_empty());
        assert!(!info.exported_symbols.is_empty());

        let symbol_names: Vec<&str> = info.exported_symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(symbol_names.contains(&"greet"), "Should extract 'greet' function, got: {:?}", symbol_names);
        assert!(symbol_names.contains(&"Greeter"), "Should extract 'Greeter' class, got: {:?}", symbol_names);
    }

    #[test]
    fn test_analyze_unknown_extension() {
        let info = analyze_file_ast(Path::new("test.xyz"), "some content");
        assert!(info.boundaries.is_empty());
        assert!(info.exported_symbols.is_empty());
    }

    #[test]
    fn test_large_impl_splits_at_methods() {
        // Build an impl block that exceeds MAX_BLOCK_LINES_BEFORE_SPLIT
        let mut lines = vec!["impl BigStruct {".to_string()];
        for i in 0..10 {
            lines.push(format!("    pub fn method_{i}(&self) -> i32 {{"));
            for j in 0..8 {
                lines.push(format!("        let _x{j} = {j};"));
            }
            lines.push("        0".to_string());
            lines.push("    }".to_string());
            lines.push(String::new());
        }
        lines.push("}".to_string());

        let content = format!("pub struct BigStruct {{}}\n\n{}", lines.join("\n"));
        let info = analyze_file_ast(Path::new("test.rs"), &content);

        let method_boundaries: Vec<_> = info.boundaries.iter()
            .filter(|b| b.kind == "method")
            .collect();
        assert!(
            !method_boundaries.is_empty(),
            "large impl should produce method-level boundaries"
        );
        assert!(
            method_boundaries.iter().any(|b| b.name.contains("BigStruct::method_0")),
            "method boundaries should be qualified with parent name"
        );
    }

    #[test]
    fn test_small_impl_no_method_split() {
        let content = r#"
impl SmallStruct {
    pub fn one(&self) -> i32 { 1 }
    pub fn two(&self) -> i32 { 2 }
}
"#;
        let info = analyze_file_ast(Path::new("test.rs"), content);
        let method_boundaries: Vec<_> = info.boundaries.iter()
            .filter(|b| b.kind == "method")
            .collect();
        assert!(
            method_boundaries.is_empty(),
            "small impl should not produce method-level boundaries"
        );
    }
}
