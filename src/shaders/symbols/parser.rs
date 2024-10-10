use std::{path::{Path, PathBuf}, vec};

use log::error;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor, QueryMatch, Tree, TreeCursor};

use crate::shaders::symbols::symbols::{
    ShaderPosition, ShaderRange,
    ShaderSymbolList,
};

use super::{glsl_parser::{GlslFunctionTreeParser, GlslIncludeTreeParser, GlslStructTreeParser, GlslVariableTreeParser}, hlsl_parser::{HlslFunctionTreeParser, HlslIncludeTreeParser, HlslStructTreeParser, HlslVariableTreeParser}, symbols::ShaderScope};

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
    parser: Parser,
    symbol_parsers: Vec<Box<dyn SymbolTreeParser>>,
}
impl SymbolParser {
    pub fn hlsl() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_hlsl::language())
            .expect("Error loading HLSL grammar");
        Self {
            parser,
            language: tree_sitter_hlsl::language(),
            symbol_parsers: vec![
                Box::new(HlslFunctionTreeParser{}),
                Box::new(HlslStructTreeParser{}),
                Box::new(HlslVariableTreeParser{}),
                Box::new(HlslIncludeTreeParser{}),
            ]
        }
    }
    pub fn glsl() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_glsl::language())
            .expect("Error loading GLSL grammar");
        Self {
            parser,
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
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_glsl::language())//TODO: tree_sitter_wgsl_bevy::language(),
            .expect("Error loading WGSL grammar");
        Self {
            parser,
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
    fn get_tree(&mut self, _file_path: &Path, shader_content: &str) -> Option<Tree> {
        // TODO: handle old tree for perfs using a LRU cache based on file_path.
        match self.parser.parse(shader_content, None) {
            Some(tree) => Some(tree),
            None => None,
        }
    }
    pub fn query_local_symbols(&mut self, file_path: &Path, shader_content: &str) -> ShaderSymbolList {
        match self.get_tree(file_path, shader_content) {
            Some(tree) => {
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
            },
            None => {
                error!("Failed to parse tree for file {}", file_path.display());
                ShaderSymbolList::default()
            },
        }
    }
    pub fn find_label_at_position(
        &mut self,
        shader_content: &String,
        file_path: &Path,
        position: ShaderPosition,
    ) -> Option<(String, ShaderRange)> {
        match self.get_tree(file_path, shader_content) {
            Some(tree) => self.find_label_at_position_in_node(shader_content, file_path, tree.root_node(), position),
            None => {
                error!("Failed to parse tree for file {}", file_path.display());
                None
            },
        }
    }
    fn find_label_at_position_in_node(
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
                        match self.find_label_at_position_in_node(shader_content, file_path, child, position.clone()) {
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
}

#[allow(dead_code)] // Debug
fn print_debug_cursor(cursor: &mut TreeCursor, depth: usize) {
    loop {
        error!("{}\"{}\": \"{}\"", " ".repeat(depth * 2), cursor.field_name().unwrap_or("None"), cursor.node().kind());
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