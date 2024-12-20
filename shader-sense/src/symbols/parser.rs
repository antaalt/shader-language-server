use std::{
    path::{Path, PathBuf},
    vec,
};

use tree_sitter::{InputEdit, Node, Parser, QueryCursor, QueryMatch, Tree, TreeCursor};

use crate::symbols::symbols::{ShaderPosition, ShaderRange, ShaderSymbolList};

use super::{
    glsl_parser::{
        GlslDefineTreeParser, GlslFunctionTreeParser, GlslIncludeTreeParser, GlslStructTreeParser,
        GlslVariableTreeParser,
    },
    hlsl_parser::{
        HlslDefineTreeParser, HlslFunctionTreeParser, HlslIncludeTreeParser, HlslStructTreeParser,
        HlslVariableTreeParser,
    },
    symbols::{ShaderScope, SymbolError},
};

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
    fn get_query(&self) -> String;
    // Process the match & convert it to symbol
    fn process_match(
        &self,
        matches: QueryMatch,
        file_path: &Path,
        shader_content: &str,
        scopes: &Vec<ShaderScope>,
        symbols: &mut ShaderSymbolList,
    );
    fn compute_scope_stack(
        &self,
        scopes: &Vec<ShaderScope>,
        range: &ShaderRange,
    ) -> Vec<ShaderScope> {
        scopes
            .iter()
            .filter_map(|e| {
                if e.contain_bounds(&range) {
                    Some(e.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<ShaderScope>>()
    }
}

#[derive(Debug, Clone)]
pub struct SymbolTree {
    pub file_path: PathBuf,
    pub content: String,
    tree: Tree,
}
pub struct SymbolParser {
    parser: Parser,
    symbol_parsers: Vec<(Box<dyn SymbolTreeParser>, tree_sitter::Query)>,
    scope_query: tree_sitter::Query,
}

fn create_symbol_parser(
    symbol_parser: Box<dyn SymbolTreeParser>,
    language: &tree_sitter::Language,
) -> (Box<dyn SymbolTreeParser>, tree_sitter::Query) {
    let query =
        tree_sitter::Query::new(language.clone(), symbol_parser.get_query().as_str()).unwrap();
    (symbol_parser, query)
}

impl SymbolParser {
    pub fn hlsl() -> Self {
        let lang = tree_sitter_hlsl::language();
        let mut parser = Parser::new();
        parser
            .set_language(lang.clone())
            .expect("Error loading HLSL grammar");
        Self {
            parser,
            symbol_parsers: vec![
                create_symbol_parser(Box::new(HlslFunctionTreeParser { is_field: false }), &lang),
                create_symbol_parser(Box::new(HlslStructTreeParser::new()), &lang),
                create_symbol_parser(Box::new(HlslVariableTreeParser { is_field: false }), &lang),
                create_symbol_parser(Box::new(HlslIncludeTreeParser {}), &lang),
                create_symbol_parser(Box::new(HlslDefineTreeParser {}), &lang),
            ],
            scope_query: tree_sitter::Query::new(lang.clone(), r#"(compound_statement) @scope"#)
                .unwrap(),
        }
    }
    pub fn glsl() -> Self {
        let lang = tree_sitter_glsl::language();
        let mut parser = Parser::new();
        parser
            .set_language(lang.clone())
            .expect("Error loading GLSL grammar");
        Self {
            parser,
            symbol_parsers: vec![
                create_symbol_parser(Box::new(GlslFunctionTreeParser {}), &lang),
                create_symbol_parser(Box::new(GlslStructTreeParser {}), &lang),
                create_symbol_parser(Box::new(GlslVariableTreeParser {}), &lang),
                create_symbol_parser(Box::new(GlslIncludeTreeParser {}), &lang),
                create_symbol_parser(Box::new(GlslDefineTreeParser {}), &lang),
            ],
            scope_query: tree_sitter::Query::new(lang.clone(), r#"(compound_statement) @scope"#)
                .unwrap(),
        }
    }
    pub fn wgsl() -> Self {
        let lang = tree_sitter_wgsl_bevy::language();
        let mut parser = Parser::new();
        parser
            .set_language(lang.clone())
            .expect("Error loading WGSL grammar");
        Self {
            parser,
            symbol_parsers: vec![],
            scope_query: tree_sitter::Query::new(lang.clone(), r#"(compound_statement) @scope"#)
                .unwrap(),
        }
    }
    fn query_scopes(
        &self,
        file_path: &Path,
        shader_content: &str,
        tree: &Tree,
    ) -> Vec<ShaderScope> {
        // TODO: look for namespace aswell
        let mut query_cursor = QueryCursor::new();
        let mut scopes = Vec::new();
        for matche in query_cursor.matches(
            &self.scope_query,
            tree.root_node(),
            shader_content.as_bytes(),
        ) {
            scopes.push(ShaderScope::from_range(
                matche.captures[0].node.range(),
                file_path.into(),
            ));
        }
        scopes
    }
    pub fn create_ast(
        &mut self,
        file_path: &Path,
        shader_content: &str,
    ) -> Result<SymbolTree, SymbolError> {
        match self.parser.parse(shader_content, None) {
            Some(tree) => Ok(SymbolTree {
                file_path: file_path.into(),
                content: shader_content.into(),
                tree,
            }),
            None => Err(SymbolError::ParseError(format!(
                "Failed to parse AST for file {}",
                file_path.display()
            ))),
        }
    }
    pub fn update_ast(
        &mut self,
        symbol_tree: &mut SymbolTree,
        new_shader_content: &str,
        range: tree_sitter::Range,
        new_text: &String,
    ) -> Result<(), SymbolError> {
        let line_count = new_text.lines().count();
        symbol_tree.tree.edit(&InputEdit {
            start_byte: range.start_byte,
            old_end_byte: range.end_byte,
            new_end_byte: range.start_byte + new_text.len(),
            start_position: range.start_point,
            old_end_position: range.end_point,
            new_end_position: tree_sitter::Point {
                row: if line_count == 0 {
                    range.start_point.row + new_text.len()
                } else {
                    new_text.lines().last().as_slice().len()
                },
                column: range.start_point.column + line_count,
            },
        });
        // Update the tree.
        match self
            .parser
            .parse(new_shader_content, Some(&symbol_tree.tree))
        {
            Some(new_tree) => {
                symbol_tree.tree = new_tree;
                symbol_tree.content = new_shader_content.into();
                Ok(())
            }
            None => Err(SymbolError::ParseError(format!(
                "Failed to update AST for file {}.",
                symbol_tree.file_path.display()
            ))),
        }
    }
    pub fn query_local_symbols(
        &self,
        symbol_tree: &SymbolTree,
    ) -> Result<ShaderSymbolList, SymbolError> {
        let scopes = self.query_scopes(
            &symbol_tree.file_path,
            &symbol_tree.content,
            &symbol_tree.tree,
        );
        let mut symbols = ShaderSymbolList::default();
        for parser in &self.symbol_parsers {
            let mut query_cursor = QueryCursor::new();
            for matches in query_cursor.matches(
                &parser.1,
                symbol_tree.tree.root_node(),
                symbol_tree.content.as_bytes(),
            ) {
                parser.0.process_match(
                    matches,
                    &symbol_tree.file_path,
                    &symbol_tree.content,
                    &scopes,
                    &mut symbols,
                );
            }
        }
        Ok(symbols)
    }
    pub fn find_label_at_position(
        &self,
        symbol_tree: &SymbolTree,
        position: ShaderPosition,
    ) -> Result<(String, ShaderRange), SymbolError> {
        self.find_label_at_position_in_node(symbol_tree, symbol_tree.tree.root_node(), position)
    }
    pub fn find_label_chain_at_position(
        &mut self,
        symbol_tree: &SymbolTree,
        position: ShaderPosition,
    ) -> Result<Vec<(String, ShaderRange)>, SymbolError> {
        self.find_label_chain_at_position_in_node(
            symbol_tree,
            symbol_tree.tree.root_node(),
            position,
        )
    }
    fn find_label_at_position_in_node(
        &self,
        symbol_tree: &SymbolTree,
        node: Node,
        position: ShaderPosition,
    ) -> Result<(String, ShaderRange), SymbolError> {
        fn range_contain(including_range: tree_sitter::Range, position: ShaderPosition) -> bool {
            let including_range =
                ShaderRange::from_range(including_range, position.file_path.clone());
            including_range.contain(&position)
        }
        if range_contain(node.range(), position.clone()) {
            match node.kind() {
                // identifier = function name, variable...
                // type_identifier = struct name, class name...
                // primitive_type = float, uint...
                // string_content = include, should check preproc_include as parent.
                // TODO: should depend on language...
                "identifier" | "type_identifier" | "primitive_type" => {
                    return Ok((
                        get_name(&symbol_tree.content, node).into(),
                        ShaderRange::from_range(node.range(), symbol_tree.file_path.clone()),
                    ))
                }
                // TODO: should use string_content instead
                "string_literal" => {
                    let path = get_name(&symbol_tree.content, node);
                    return Ok((
                        path[1..path.len() - 1].into(),
                        ShaderRange::from_range(node.range(), symbol_tree.file_path.clone()),
                    ));
                }
                _ => {
                    for child in node.children(&mut node.walk()) {
                        match self.find_label_at_position_in_node(
                            symbol_tree,
                            child,
                            position.clone(),
                        ) {
                            Ok(label) => return Ok(label),
                            Err(err) => {
                                if let SymbolError::NoSymbol = err {
                                    // Skip.
                                } else {
                                    return Err(err);
                                }
                            }
                        }
                    }
                }
            }
            Err(SymbolError::NoSymbol)
        } else {
            Err(SymbolError::NoSymbol)
        }
    }
    fn find_label_chain_at_position_in_node(
        &self,
        symbol_tree: &SymbolTree,
        node: Node,
        position: ShaderPosition,
    ) -> Result<Vec<(String, ShaderRange)>, SymbolError> {
        fn range_contain(including_range: tree_sitter::Range, position: ShaderPosition) -> bool {
            let including_range =
                ShaderRange::from_range(including_range, position.file_path.clone());
            including_range.contain(&position)
        }
        if range_contain(node.range(), position.clone()) {
            match node.kind() {
                "identifier" => {
                    return Ok(vec![(
                        get_name(&symbol_tree.content, node).into(),
                        ShaderRange::from_range(node.range(), symbol_tree.file_path.clone()),
                    )])
                }
                "field_identifier" => {
                    let mut chain = Vec::new();
                    let mut current_node = node.prev_named_sibling().unwrap();
                    loop {
                        let field = current_node.next_named_sibling().unwrap();
                        if field.kind() == "field_identifier" {
                            chain.push((
                                get_name(&symbol_tree.content, field).into(),
                                ShaderRange::from_range(
                                    field.range(),
                                    symbol_tree.file_path.clone(),
                                ),
                            ));
                        } else {
                            return Err(SymbolError::InternalErr(format!(
                                "Unhandled case in find_label_chain_at_position_in_node: {}",
                                field.kind()
                            )));
                        }
                        match current_node.child_by_field_name("argument") {
                            Some(child) => {
                                current_node = child;
                            }
                            None => {
                                let identifier = current_node;
                                chain.push((
                                    get_name(&symbol_tree.content, identifier).into(),
                                    ShaderRange::from_range(
                                        identifier.range(),
                                        symbol_tree.file_path.clone(),
                                    ),
                                ));
                                break;
                            } // Should have already break here
                        }
                    }
                    return Ok(chain);
                }
                _ => {
                    for child in node.children(&mut node.walk()) {
                        match self.find_label_chain_at_position_in_node(
                            symbol_tree,
                            child,
                            position.clone(),
                        ) {
                            Ok(chain_list) => return Ok(chain_list),
                            Err(err) => {
                                if let SymbolError::NoSymbol = err {
                                    // Skip.
                                } else {
                                    return Err(err);
                                }
                            }
                        }
                    }
                }
            }
            Err(SymbolError::NoSymbol)
        } else {
            Err(SymbolError::NoSymbol)
        }
    }
}

#[allow(dead_code)] // Debug
fn print_debug_cursor(cursor: &mut TreeCursor, depth: usize) -> String {
    let mut debug_tree = String::new();
    loop {
        debug_tree.push_str(&format!(
            "{}\"{}\": \"{}\"",
            " ".repeat(depth * 2),
            cursor.field_name().unwrap_or("None"),
            cursor.node().kind()
        ));
        if cursor.goto_first_child() {
            print_debug_cursor(cursor, depth + 1);
            cursor.goto_parent();
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    debug_tree
}
#[allow(dead_code)] // Debug
fn print_debug_tree(tree: Tree) -> String {
    print_debug_cursor(&mut tree.root_node().walk(), 0)
}
