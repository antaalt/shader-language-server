use std::path::{Path, PathBuf};

use tree_sitter::{Node, Query, QueryCursor, Tree};

use crate::shaders::symbols::symbols::{
    ShaderMembers, ShaderParameter, ShaderPosition, ShaderRange, ShaderSignature, ShaderSymbolList,
};

use super::symbols::{ShaderScope, ShaderSymbol};

fn get_name<'a>(shader_content: &'a str, node: Node) -> &'a str {
    let range = node.range();
    &shader_content[range.start_byte..range.end_byte]
}

impl ShaderRange {
    fn from_range(value: tree_sitter::Range, file_path: PathBuf) -> Self {
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

fn query_scopes(file_path: &Path, shader_content: &str, tree: &Tree) -> Vec<ShaderScope> {
    // TODO: look for namespace aswell
    const SCOPE_QUERY: &'static str = r#"body: (compound_statement) @scope"#;
    let query =
        Query::new(&tree_sitter_glsl::language(), SCOPE_QUERY).expect("Failed to query scope");
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

fn query_function(
    file_path: &Path,
    shader_content: &str,
    node: Node,
    scopes: Vec<ShaderScope>,
) -> ShaderSymbolList {
    const FUNCTION_QUERY: &'static str = r#"(function_definition
    type: (type_identifier) @function.return
    declarator: (function_declarator
        declarator: (identifier) @function.label
        parameters: (parameter_list 
            ((parameter_declaration
                type: (_) @function.param.type
                declarator: (_) @function.param.decl
            )(",")?)+
        )
    )
    body: (compound_statement) @function.scope
    )"#; // compound_statement is function scope.
    let query = Query::new(&tree_sitter_glsl::language(), FUNCTION_QUERY)
        .expect("Failed to query function");
    let mut query_cursor = QueryCursor::new();

    let mut symbols = ShaderSymbolList::default();
    for matche in query_cursor.matches(&query, node, shader_content.as_bytes()) {
        let scope_node = matche.captures[matche.captures.len() - 1].node;
        let range = ShaderRange::from_range(scope_node.range(), file_path.into());
        let scope_stack = scopes
            .iter()
            .filter_map(|e| {
                if e.contain_bounds(&range) {
                    Some(e.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<ShaderScope>>();
        // Query internal scopes variables
        /*let content_scope_stack = {
            let mut s = scope_stack.clone();
            s.push(range.clone());
            s
        };
        query_variables(file_path, &shader_content[scope_node.range().start_byte.. scope_node.range().end_byte], scope_node, {
            let mut s = scope_stack.clone();
            s.push(range.clone());
            s
        });*/
        symbols.functions.push(ShaderSymbol {
            label: get_name(shader_content, matche.captures[1].node).into(),
            description: "".into(),
            version: "".into(),
            stages: vec![],
            link: None,
            members: None,
            signature: Some(ShaderSignature {
                returnType: get_name(shader_content, matche.captures[0].node).into(),
                description: "".into(),
                parameters: matche.captures[2..matche.captures.len() - 1]
                    .chunks(2)
                    .map(|w| ShaderParameter {
                        ty: get_name(shader_content, w[0].node).into(),
                        label: get_name(shader_content, w[1].node).into(),
                        description: "".into(),
                    })
                    .collect::<Vec<ShaderParameter>>(),
            }),
            ty: None,
            range: Some(ShaderRange::from_range(
                matche.captures[0].node.range(),
                file_path.into(),
            )),
            scope_stack: Some(scope_stack), // In GLSL, all function are global scope.
        });
    }
    symbols
}

fn query_struct(
    file_path: &Path,
    shader_content: &str,
    node: Node,
    _scopes: Vec<ShaderScope>,
) -> ShaderSymbolList {
    const STRUCT_QUERY: &'static str = r#"(struct_specifier
    name: (type_identifier) @struct.type
    body: (field_declaration_list
        (field_declaration 
            type: (_) @struct.param.type
            declarator: (_) @struct.param.decl
        )+
    )
    )"#;
    let query =
        Query::new(&tree_sitter_glsl::language(), STRUCT_QUERY).expect("Failed to query struct");
    let mut query_cursor = QueryCursor::new();

    let mut symbols = ShaderSymbolList::default();
    for matche in query_cursor.matches(&query, node, shader_content.as_bytes()) {
        symbols.types.push(ShaderSymbol {
            label: get_name(shader_content, matche.captures[0].node).into(),
            description: "".into(),
            version: "".into(),
            stages: vec![],
            link: None,
            members: Some(ShaderMembers {
                members: matche.captures[1..]
                    .chunks(2)
                    .map(|w| ShaderParameter {
                        ty: get_name(shader_content, w[0].node).into(),
                        label: get_name(shader_content, w[1].node).into(),
                        description: "".into(),
                    })
                    .collect::<Vec<ShaderParameter>>(),
                methods: vec![],
            }),
            signature: None,
            ty: None,
            range: Some(ShaderRange::from_range(
                matche.captures[0].node.range(),
                file_path.into(),
            )),
            scope_stack: None,
        });
    }
    symbols
}

fn query_variables(
    file_path: &Path,
    shader_content: &str,
    node: Node,
    _scopes: Vec<ShaderScope>,
) -> ShaderSymbolList {
    const STRUCT_QUERY: &'static str = r#"(declaration
    type: (type_identifier) @variable.type
    declarator: [(init_declarator
        declarator: (identifier) @variable.label
        value: (_) @variable.value
    ) 
    (identifier) @variable.label
    ]
    )"#;
    let query =
        Query::new(&tree_sitter_glsl::language(), STRUCT_QUERY).expect("Failed to query struct");
    let mut query_cursor = QueryCursor::new();

    let mut symbols = ShaderSymbolList::default();
    for matche in query_cursor.matches(&query, node, shader_content.as_bytes()) {
        // Check if its parameter or struct element.
        let _type_qualifier = get_name(shader_content, matche.captures[0].node);
        // TODO: handle values & qualifiers..
        //let _value = get_name(shader_content, matche.captures[2].node);
        symbols.variables.push(ShaderSymbol {
            label: get_name(shader_content, matche.captures[1].node).into(),
            description: "".into(),
            version: "".into(),
            stages: vec![],
            link: None,
            members: None,
            signature: None,
            ty: Some(get_name(shader_content, matche.captures[0].node).into()),
            range: Some(ShaderRange::from_range(
                matche.captures[1].node.range(),
                file_path.into(),
            )),
            scope_stack: None,
        });
    }
    symbols
}
pub fn query_symbols(file_path: &Path, shader_content: &str, tree: Tree) -> ShaderSymbolList {
    let scopes = query_scopes(file_path, shader_content, &tree);
    let mut symbols = ShaderSymbolList::default();
    symbols.append(query_function(
        file_path,
        shader_content,
        tree.root_node(),
        scopes.clone(),
    ));
    symbols.append(query_struct(
        file_path,
        shader_content,
        tree.root_node(),
        scopes.clone(),
    ));
    symbols.append(query_variables(
        file_path,
        shader_content,
        tree.root_node(),
        scopes.clone(),
    ));
    symbols
}

pub fn find_symbol_at_position(
    file_path: &Path,
    shader_content: &String,
    tree: Tree,
    position: ShaderPosition,
) -> Option<ShaderSymbol> {
    let symbols = query_symbols(file_path, shader_content, tree);
    for symbol_list in symbols {
        match symbol_list.0.iter().find(|e| match &e.range {
            Some(range) => range.contain(&position),
            None => false,
        }) {
            Some(symbol) => return Some(symbol.clone()),
            None => {}
        }
    }
    None
}

#[allow(dead_code)] // Debug
fn print_debug_tree(tree: Tree) {
    fn print_node(node: Node, depth: usize) {
        println!(
            "{}{}: {}",
            " ".repeat(depth * 2),
            node.kind(),
            node.grammar_name()
        );
        for child in node.children(&mut node.walk()) {
            print_node(child, depth + 1);
        }
    }
    print_node(tree.root_node(), 0);
}
