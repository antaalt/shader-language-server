use std::{path::{Path, PathBuf}, vec};

use log::error;
use tree_sitter::{Language, Node, Query, QueryCursor, QueryMatch, Tree, TreeCursor};

use crate::shaders::symbols::symbols::{
    ShaderPosition, ShaderRange,
    ShaderSymbolList,
};

use super::{glsl_parser::{GlslFunctionTreeParser, GlslIncludeTreeParser, GlslStructTreeParser, GlslVariableTreeParser}, hlsl_parser::HlslFunctionTreeParser, symbols::{ShaderScope, ShaderSymbol}};

pub(super) fn get_name<'a>(shader_content: &'a str, node: Node) -> &'a str {
    let range = node.range();
    &shader_content[range.start_byte..range.end_byte]
}

impl ShaderRange {
    pub(super) fn from_range(value: tree_sitter::Range, file_path: PathBuf) -> Self {
        ShaderRange {
            start: ShaderPosition {
                file_path: file_path.clone(),
                line: value.start_point.row as u32,
                pos: value.start_point.column as u32,
            },
            end: ShaderPosition {
                file_path: file_path.clone(),
                line: value.end_point.row as u32,
                pos: value.end_point.column as u32,
            },
        }
    }
}

pub trait SymbolTreeParser {
    // The query to match tree node
    fn get_query(&self) -> &str;
    // Process the match & convert it to symbol
    fn process_match(&self, matches: QueryMatch, file_path: &Path, shader_content: &str, scopes: &Vec<ShaderScope>, symbols: &mut ShaderSymbolList);
    fn compute_scope_stack(&self, scopes: &Vec<ShaderScope>, range: &ShaderRange) -> Vec<ShaderScope> {
        scopes.iter().filter_map(|e| if e.contain_bounds(&range) {
            Some(e.clone())
        } else {
            None
        }).collect::<Vec<ShaderScope>>()
    }
}
pub struct SymbolParser {
    language: Language,
    symbol_parsers: Vec<Box<dyn SymbolTreeParser>>,
}
impl SymbolParser {
    pub fn hlsl() -> Self {
        Self {
            language: tree_sitter_hlsl::language(),
            symbol_parsers: vec![
                Box::new(HlslFunctionTreeParser{})
            ]
        }
    }
    pub fn glsl() -> Self {
        Self {
            language: tree_sitter_glsl::language(),
            symbol_parsers: vec![
                Box::new(GlslFunctionTreeParser{}),
                Box::new(GlslStructTreeParser{}),
                Box::new(GlslVariableTreeParser{}),
                Box::new(GlslIncludeTreeParser{}),
            ]
        }
    }
    pub fn wgsl() -> Self {
        Self {
            language: tree_sitter_glsl::language(),//TODO: tree_sitter_wgsl_bevy::language(),
            symbol_parsers: vec![]
        }
    }
    fn query_scopes(&self, file_path: &Path, shader_content: &str, tree: &Tree) -> Vec<ShaderScope> {
        // TODO: look for namespace aswell
        // TODO: This should be per lang aswell...
        const SCOPE_QUERY: &'static str = r#"body: (compound_statement) @scope"#;
        let query =
            Query::new(&self.language, SCOPE_QUERY).expect("Failed to query scope");
        let mut query_cursor = QueryCursor::new();

        let mut scopes = Vec::new();
        for matche in query_cursor.matches(&query, tree.root_node(), shader_content.as_bytes()) {
            scopes.push(ShaderScope::from_range(
                matche.captures[0].node.range(),
                file_path.into(),
            ));
        }
        scopes
    }
    pub fn query_local_symbols(&self, file_path: &Path, shader_content: &str, tree: Tree) -> ShaderSymbolList {
        let scopes = self.query_scopes(file_path, shader_content, &tree);
        let mut symbols = ShaderSymbolList::default();
        for parser in &self.symbol_parsers {
            let query = Query::new(&self.language, parser.get_query())
                .expect("Invalid query");
            let mut query_cursor = QueryCursor::new();

            for matches in query_cursor.matches(&query, tree.root_node(), shader_content.as_bytes()) {
                parser.process_match(matches, file_path, shader_content, &scopes, &mut symbols);
            }
        }
        symbols
    }
    pub fn find_label_at_position(
        &self,
        shader_content: &String,
        file_path: &Path,
        node: Node,
        position: ShaderPosition,
    ) -> Option<(String, ShaderRange)> {
        fn range_contain(including_range: tree_sitter::Range, position: ShaderPosition) -> bool {
            let including_range = ShaderRange::from_range(including_range, position.file_path.clone());
            including_range.contain(&position)
        }
        if range_contain(node.range(), position.clone()) {
            match node.kind() {
                // identifier = function name, variable...
                // type_identifier = struct name, class name...
                // primitive_type = float, uint...
                // string_content = include, should check preproc_include as parent.
                // TODO: should depend on language...
                "identifier" | "type_identifier" | "primitive_type" | "string_content" => {
                    return Some((get_name(&shader_content, node).into(), ShaderRange::from_range(node.range(), file_path.into())))
                }
                _ => {
                    for child in node.children(&mut node.walk()) {
                        match self.find_label_at_position(shader_content, file_path, child, position.clone()) {
                            Some(label) => return Some(label),
                            None => {}
                        }
                    }
                }
            }
            None
        } else {
            None
        }
    }
    pub fn find_symbol_at_position(
        &self,
        file_path: &Path,
        shader_content: &String,
        tree: Tree,
        position: ShaderPosition,
    ) -> Option<ShaderSymbol> {
        // Need to get the word at position, then use as label to find in symbols list.
        match self.find_label_at_position(shader_content, file_path, tree.root_node(), position) {
            Some((label, _range)) => {
                let all_symbols = self.query_local_symbols(file_path, shader_content, tree);
                all_symbols.find_symbol(label.into())
            }
            None => None,
        }
    }
}

#[allow(dead_code)] // Debug
fn print_debug_cursor(cursor: &mut TreeCursor, depth: usize) {
    loop {
        println!("{}\"{}\": \"{}\"", " ".repeat(depth * 2), cursor.field_name().unwrap_or("None"), cursor.node().kind());
        if cursor.goto_first_child() {
            print_debug_cursor(cursor, depth + 1);
            cursor.goto_parent();
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
}
#[allow(dead_code)] // Debug
fn print_debug_tree(tree: Tree) {
    print_debug_cursor(&mut tree.root_node().walk(), 0);
}
