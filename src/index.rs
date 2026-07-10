use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::Serialize;
use tree_sitter::{Language, Node, Parser};

#[derive(Debug, Clone, Serialize)]
pub struct SymbolRef {
    pub path: String,
    pub line: u32,
    pub col: u32,
    pub context: String,
}

#[derive(Debug, Default, Serialize)]
pub struct Lookup {
    pub declarations: Vec<SymbolRef>,
    pub references: Vec<SymbolRef>,
}

#[derive(Default)]
pub struct SymbolIndex {
    decls: HashMap<String, Vec<SymbolRef>>,
    refs: HashMap<String, Vec<SymbolRef>>,
}

fn language_for(path: &str) -> Option<Language> {
    let ext = path.rsplit('.').next()?.to_lowercase();
    let lang: Language = match ext.as_str() {
        "java" => tree_sitter_java::LANGUAGE.into(),
        "kt" | "kts" => tree_sitter_kotlin_ng::LANGUAGE.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        "py" => tree_sitter_python::LANGUAGE.into(),
        "js" | "jsx" | "mjs" => tree_sitter_javascript::LANGUAGE.into(),
        "ts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        _ => return None,
    };
    Some(lang)
}

fn is_identifier_kind(kind: &str) -> bool {
    matches!(
        kind,
        "identifier"
            | "simple_identifier"
            | "type_identifier"
            | "field_identifier"
            | "property_identifier"
            | "shorthand_property_identifier"
    )
}

impl SymbolIndex {
    /// Parse `paths` (relative to `root`) and index identifier declarations
    /// and references. Files without a supported grammar are skipped.
    pub fn build(root: &Path, paths: &[String]) -> SymbolIndex {
        let mut idx = SymbolIndex::default();
        for path in paths {
            let Some(lang) = language_for(path) else { continue };
            let Ok(source) = fs::read_to_string(root.join(path)) else { continue };
            idx.index_file(path, &source, &lang);
        }
        idx
    }

    pub fn lookup(&self, name: &str) -> Lookup {
        Lookup {
            declarations: self.decls.get(name).cloned().unwrap_or_default(),
            references: self.refs.get(name).cloned().unwrap_or_default(),
        }
    }

    fn index_file(&mut self, path: &str, source: &str, lang: &Language) {
        let mut parser = Parser::new();
        if parser.set_language(lang).is_err() {
            return;
        }
        let Some(tree) = parser.parse(source, None) else { return };
        let lines: Vec<&str> = source.lines().collect();
        self.walk(tree.root_node(), path, source.as_bytes(), &lines);
    }

    fn walk(&mut self, node: Node, path: &str, src: &[u8], lines: &[&str]) {
        if is_identifier_kind(node.kind()) {
            if let Ok(name) = node.utf8_text(src) {
                let pos = node.start_position();
                let sym = SymbolRef {
                    path: path.to_string(),
                    line: pos.row as u32 + 1,
                    col: pos.column as u32,
                    context: lines.get(pos.row).map(|l| l.trim().to_string()).unwrap_or_default(),
                };
                // an identifier is a declaration iff it fills the `name` slot
                // of a declaration-like parent; call/access nodes (e.g. Java
                // method_invocation) also have a `name` field and must not count
                let is_decl = node.parent().is_some_and(|p| {
                    let k = p.kind();
                    (k.contains("declaration")
                        || k.contains("definition")
                        || k.contains("declarator")
                        || k == "type_spec")
                        && p.child_by_field_name("name")
                            .is_some_and(|n| n.id() == node.id())
                });
                let map = if is_decl { &mut self.decls } else { &mut self.refs };
                map.entry(name.to_string()).or_default().push(sym);
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk(child, path, src, lines);
        }
    }
}
