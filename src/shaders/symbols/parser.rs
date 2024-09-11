use std::path::Path;

use log::error;
use tree_sitter::{Node, Point, Range, Tree};

use crate::shaders::symbols::symbols::{ShaderMembers, ShaderParameter, ShaderPosition, ShaderRange, ShaderSignature, ShaderSymbolList};

use super::symbols::{ShaderScope, ShaderSymbol};


struct ScopeParser {
    scope_stack: Vec<Point>,
    scopes: Vec<ShaderScope>,
}

fn get_name<'a>(shader_content: &'a str, node : Node) -> &'a str {
    let range = node.range();
    &shader_content[range.start_byte..range.end_byte]
}
fn get_child<'a>(node : Node<'a>, kind: &str) -> Option<Node<'a>> {
    node.children(&mut node.walk()).find(|e| e.kind() == kind)
}


fn parse_function<F : FnMut(Node) -> ShaderSymbolList>(file_path: &Path, node: Node, shader_content: &str, child_filter: &mut F) -> ShaderSymbolList {
    let mut symbols = ShaderSymbolList::default();
    let mut signature = ShaderSignature {
        returnType: "".into(),
        description: "".into(),
        parameters: vec![],
    };
    let mut symbol = ShaderSymbol {
        label: "".into(),
        description: "".into(),
        version: "".into(),
        stages: vec![],
        link: None,
        signature: None,
        members: None,
        ty: None,
        range: None,
        scope_stack: None,
    };
    for function_child in node.children(&mut node.walk()) {
        match function_child.kind() {
            "type_identifier" | "primitive_type" => {
                signature.returnType = String::from(get_name(shader_content, function_child));
                signature.description = "".into();
            }
            "function_declarator" => {
                // Label + params
                for declarator_child in function_child.children(&mut function_child.walk()) {
                    match declarator_child.kind() {
                        "identifier" => {
                            symbol.label = String::from(get_name(shader_content, declarator_child));
                            symbol.description = "User defined function".into();
                            symbol.range = Some(ShaderRange::new(
                                ShaderPosition::new(file_path.into(), declarator_child.start_position().row as u32, declarator_child.start_position().column as u32), 
                                ShaderPosition::new(file_path.into(), declarator_child.end_position().row as u32, declarator_child.end_position().column as u32)
                            ));
                        }
                        "parameter_list" => {
                            let mut parameter_index = 0;
                            for parameter_child in declarator_child.children(&mut declarator_child.walk()) {
                                match parameter_child.kind() {
                                    "parameter_declaration" => {
                                        let mut parameter = ShaderParameter {
                                            ty: "".into(),
                                            label: "".into(),
                                            description: "".into(),
                                        };
                                        for variable_child in parameter_child.children(&mut parameter_child.walk()) {
                                            match variable_child.kind() {
                                                // Should be either primitive_type or type_identifier.
                                                "primitive_type" => parameter.ty = get_name(shader_content, variable_child).into(),
                                                "type_identifier" => parameter.ty = get_name(shader_content, variable_child).into(),
                                                // Label
                                                "identifier" => parameter.label = get_name(shader_content, variable_child).into(),
                                                // Optional semantic
                                                "semantics" => {},
                                                "array_declarator" => {
                                                    for array_variable_child in variable_child.children(&mut variable_child.walk()) {
                                                        match array_variable_child.kind() {
                                                            "identifier" => parameter.label = get_name(shader_content, array_variable_child).into(),
                                                            "number_literal" => parameter.label = format!("{}[{}]", parameter.label, get_name(shader_content, array_variable_child)),
                                                            "[" | "]" => {} // Ignore
                                                            _ => error!("Unhandled array_parameter_variable_child {} ({})", array_variable_child.kind(), get_name(shader_content, array_variable_child))
                                                        }
                                                    }
                                                }
                                                _ => error!("Unhandled parameter_variable_child {} ({})", variable_child.kind(), get_name(shader_content, variable_child))
                                            }
                                        }
                                        parameter.description = format!("Parameter {}", parameter_index);
                                        parameter_index += 1;
                                        signature.parameters.push(parameter);
                                    }
                                    "," | ")" | "(" => {} // Ignore
                                    _ => error!("Unhandled parameter_child {} ({})", parameter_child.kind(), get_name(shader_content, parameter_child))
                                }
                            }
                        }
                        "semantics" => {
                            // TODO: handle semantics ?
                        }
                        _ => error!("Unhandled declarator_child {} ({})", declarator_child.kind(), get_name(shader_content, declarator_child))
                    }
                }
            }
            "compound_statement" => {
                // Recurse function body for variables being filtered.
                symbols.append(child_filter(function_child));
            }
            _ => error!("Unhandled function_child {} ({})", function_child.kind(), get_name(shader_content, function_child))
        }
    }
    symbol.signature = Some(signature);
    assert!(!symbol.label.is_empty() && !symbol.description.is_empty() && symbol.range.is_some() && match &symbol.signature {
        Some(signature) => 
            !signature.returnType.is_empty() && 
            signature.parameters.iter().filter(|e| !e.label.is_empty() && !e.ty.is_empty()).count() == signature.parameters.len(),
        None => false,
    }, "{:?}", symbol);
    symbols.functions.push(symbol);
    symbols
} 
fn parse_variable<F : FnMut(Node) -> ShaderSymbolList>(file_path: &Path, node: Node, shader_content: &str, child_filter: &mut F) -> ShaderSymbolList {
    let mut symbols = ShaderSymbolList::default();
    let mut symbol = ShaderSymbol {
        label: "".into(),
        description: "".into(),
        version: "".into(),
        stages: vec![],
        link: None,
        signature: None,
        members: None,
        ty: None,
        range: None,
        scope_stack: None,
    };
    for variable_child in node.children(&mut node.walk()) {
        match variable_child.kind() {
            "qualifiers" => {
                // TODO: uniform & co
            }
            "type_qualifier" => {
                // TODO: add field for qualifier.
            }
            "primitive_type" | "type_identifier" => symbol.ty = Some(get_name(shader_content, variable_child).into()),
            // Should be either 
            // - identifier (simple definition)
            // - init_declarator (identifier + declaration)
            // - array_declarator (identifier for array)
            "identifier" => {
                symbol.label = get_name(shader_content, variable_child).into();
                symbol.description = "User defined variable".into();
                symbol.range = Some(ShaderRange::new(
                    ShaderPosition::new(file_path.into(), variable_child.start_position().row as u32, variable_child.start_position().column as u32), 
                    ShaderPosition::new(file_path.into(), variable_child.end_position().row as u32, variable_child.end_position().column as u32)
                ));
            }
            "init_declarator" => {
                for init_child in variable_child.children(&mut variable_child.walk()) {
                    match init_child.kind() {
                        "identifier" => {
                            symbol.label = get_name(shader_content, init_child).into();
                            symbol.description = "User defined variable".into();
                            symbol.range = Some(ShaderRange::new(
                                ShaderPosition::new(file_path.into(), init_child.start_position().row as u32, init_child.start_position().column as u32), 
                                ShaderPosition::new(file_path.into(), init_child.end_position().row as u32, init_child.end_position().column as u32)
                            ));
                        }
                        // TODO: handle these for constant only
                        "call_expression" => {} 
                        "number_literal" => {}
                        "field_expression" => {}
                        "binary_expression" => {}
                        "parenthesized_expression" => {}
                        "conditional_expression" => {}
                        "cast_expression" => {}
                        "=" => {} // Ignore
                        _ => error!("Unhandled init_declarator {} ({})", init_child.kind(), get_name(shader_content, init_child))
                    }
                    
                }
            }
            "array_declarator" => {
                for array_declarator_child in variable_child.children(&mut variable_child.walk()) {
                    match array_declarator_child.kind() {
                        "identifier" => {
                            symbol.label = get_name(shader_content, variable_child).into();
                            symbol.description = "User defined variable".into();
                            symbol.range = Some(ShaderRange::new(
                                ShaderPosition::new(file_path.into(), array_declarator_child.start_position().row as u32, array_declarator_child.start_position().column as u32), 
                                ShaderPosition::new(file_path.into(), array_declarator_child.end_position().row as u32, array_declarator_child.end_position().column as u32)
                            ));
                        }
                        "number_literal" => {
                            // Add array
                            symbol.label = format!("{}[{}]", symbol.label, get_name(shader_content, variable_child));
                        }
                        "[" | "]" => {} // Ignore
                        _ => error!("Unhandled array_declarator {} ({})", array_declarator_child.kind(), get_name(shader_content, array_declarator_child))
                    }
                    
                }
            }
            "field_declaration_list" => {
                // Uniforms append them.
                symbols.append(child_filter(variable_child));
            }
            "semantics" => {}
            "layout_specification" => {
                // Layout (set & binding for GLSL)
            }
            "uniform" => {}
            "in" | "out" | ";" | "," | ":" => {} // Ignore
            _ => error!("Unhandled variable_child {} ({})", variable_child.kind(), get_name(shader_content, variable_child))
        }
    }
    assert!(!symbol.label.is_empty() && !symbol.description.is_empty() && symbol.range.is_some() && symbol.ty.is_some() , "{:?}", symbol);
    symbols.variables.push(symbol);
    symbols
}
fn parse_struct<F : FnMut(Node) -> ShaderSymbolList>(file_path: &Path, node: Node, shader_content: &str, _child_filter: &mut F) -> ShaderSymbolList {
    let mut symbols = ShaderSymbolList::default();
    let mut symbol = ShaderSymbol {
        label: "".into(),
        description: "".into(),
        version: "".into(),
        stages: vec![],
        link: None,
        signature: None,
        members: None,
        ty: None,
        range: None,
        scope_stack: None,
    };
    for struct_child in node.children(&mut node.walk()) {
        match struct_child.kind() {
            "struct" => {
                symbol.ty = Some("struct".into());
            }
            "type_identifier" => {
                symbol.label = get_name(shader_content, struct_child).into();
                symbol.description = "User declared structure".into();
                symbol.range = Some(ShaderRange::new(
                    ShaderPosition::new(file_path.into(), struct_child.start_position().row as u32, struct_child.start_position().column as u32), 
                    ShaderPosition::new(file_path.into(), struct_child.end_position().row as u32, struct_child.end_position().column as u32)
                ));
            }
            "field_declaration_list" => {
                let mut members = ShaderMembers::default();
                for member_child in struct_child.children(&mut struct_child.walk()) {
                    let mut param = ShaderParameter::default();
                    match member_child.kind() {
                        "field_declaration" => {
                            for decl_member_child in member_child.children(&mut member_child.walk()) {
                                match decl_member_child.kind() {
                                    "primitive_type" | "type_identifier" => {
                                        param.ty = get_name(shader_content, decl_member_child).into();
                                    }
                                    "field_identifier" => {
                                        param.label = get_name(shader_content, decl_member_child).into();
                                        param.description = format!("Member of {}", symbol.label);
                                    }
                                    "array_declarator" => {
                                        for array_declarator_child in decl_member_child.children(&mut decl_member_child.walk()) {
                                            match array_declarator_child.kind() {
                                                "identifier" | "field_identifier" => {
                                                    symbol.label = get_name(shader_content, array_declarator_child).into();
                                                    symbol.description = "User defined variable".into();
                                                    symbol.range = Some(ShaderRange::new(
                                                        ShaderPosition::new(file_path.into(), array_declarator_child.start_position().row as u32, array_declarator_child.start_position().column as u32), 
                                                        ShaderPosition::new(file_path.into(), array_declarator_child.end_position().row as u32, array_declarator_child.end_position().column as u32)
                                                    ));
                                                }
                                                "number_literal" => {
                                                    // Add array
                                                    symbol.label = format!("{}[{}]", symbol.label, get_name(shader_content, decl_member_child));
                                                }
                                                "[" | "]" => {} // Ignore
                                                _ => error!("Unhandled struct_array_declarator {} ({})", array_declarator_child.kind(), get_name(shader_content, array_declarator_child))
                                            }
                                            
                                        }
                                    }
                                    "bitfield_clause" => {
                                        // TODO: bug here, this should be semantics.
                                    }
                                    ";" => {} // Ignore
                                    _ => error!("Unhandled decl_member_child {} {}", decl_member_child.kind(), get_name(shader_content, decl_member_child))
                                }
                            }
                        }
                        "comment" | "}" | "{" => {} // Ignore
                        _ => error!("Unhanded field_declaration member{} {}", member_child.kind(), get_name(shader_content, member_child))
                    }
                    members.members.push(param);
                }
                symbol.members = Some(members);
            }
            _ => error!("Unhandled struct_child {} {}", struct_child.kind(), get_name(shader_content, struct_child))
        }
    }
    assert!(!symbol.label.is_empty() && !symbol.description.is_empty() && symbol.range.is_some() && symbol.ty.is_some() && symbol.members.is_some(), "{:?}", symbol);
    symbols.types.push(symbol);
    symbols
}
fn apply_scopes(symbols: ShaderSymbolList, scopes: &mut Vec<ShaderScope>) -> ShaderSymbolList {
    // Ensure they are sorted.
    scopes.sort_by(|a, b| a.start.cmp(&b.start));
    let scope_compute = |symbol: ShaderSymbol, scopes: &mut Vec<ShaderScope>| -> ShaderSymbol {
        let mut symbol_mut = symbol;
        let mut scope_stack = vec![];
        match &symbol_mut.range {
            Some(range) => for scope in scopes {
                if scope.contain_bounds(&range) {
                    scope_stack.push(scope.clone());
                }
            },
            None => {
                panic!("No position for {:?}", symbol_mut);
            },
        }
        symbol_mut.scope_stack = Some(scope_stack);
        symbol_mut
    };
    ShaderSymbolList {
        functions: symbols.functions.into_iter().map(|e| scope_compute(e, scopes)).collect(),
        types: symbols.types.into_iter().map(|e| scope_compute(e, scopes)).collect(),
        constants: symbols.constants.into_iter().map(|e| scope_compute(e, scopes)).collect(),
        variables: symbols.variables.into_iter().map(|e| scope_compute(e, scopes)).collect(),
        keywords: symbols.keywords.into_iter().map(|e| scope_compute(e, scopes)).collect(),
    }
}
fn parse_tree_node(file_path: &Path, shader_content: &str, node : Node, scope_parser: &mut ScopeParser) -> ShaderSymbolList {
    let mut filter_child = |child_node: Node| {
        parse_tree_node(file_path, shader_content, child_node, scope_parser)
    };
    match match node.kind() {
        "function_definition" => Some(parse_function(file_path, node, shader_content, &mut filter_child)),
        "declaration" => Some(parse_variable(file_path, node, shader_content, &mut filter_child)),
        "struct_specifier" => Some(parse_struct(file_path, node, shader_content, &mut filter_child)),
        "call_expression" => None,
        "comment" => None,
        "expression_statement" => None,
        "assignment_expression" => None,
        // Handle scopes
        "{" => {
            scope_parser.scope_stack.push(node.range().start_point);
            None
        }
        "}" => {
            if let Some(point) = scope_parser.scope_stack.pop() {
                scope_parser.scopes.push(ShaderScope {
                    start: ShaderPosition {
                        file_path: file_path.into(),
                        line: point.row as u32,
                        pos: point.column as u32,
                    },
                    end: ShaderPosition {
                        file_path: file_path.into(),
                        line: node.start_position().row as u32,
                        pos: node.start_position().column as u32,
                    }
                })
            }
            scope_parser.scope_stack.push(node.range().start_point);
            None
        }
        _ => {
            //error!("Unhandled node {} ({})", node.kind(), get_name(shader_content, node));
            // Recurse for unknown types
            let mut symbols = ShaderSymbolList::default();
            for child in node.children(&mut node.walk()) {
                symbols.append(parse_tree_node(file_path, shader_content, child, scope_parser));
            }
            Some(symbols)
        }
    } {
        Some(symbol) => symbol,
        None => ShaderSymbolList::default()
    }
}
pub fn parse_tree(path: &Path, content: &String, tree: Tree) -> ShaderSymbolList {
    let root_node = tree.root_node();
    let mut scope_parser = ScopeParser {
        scope_stack: vec![],
        scopes: vec![],
    };
    let unscoped_symbols = parse_tree_node(path, content.as_str(), root_node, &mut scope_parser);
    apply_scopes(unscoped_symbols, &mut scope_parser.scopes)
}

/*fn is_in_range(range: Range, position: Point) -> bool {
    // Check line & position bounds.
    if position.column > range.start_point.row && position.row < range.end_point.row {
        true
    } else if position.row == range.start_point.row && position.row == range.end_point.row {
        position.column > range.start_point.column && position.column < range.end_point.column
    } else if position.row == range.start_point.row && position.row < range.end_point.row {
        position.column > range.start_point.column
    } else if position.row == range.end_point.row && position.row > range.start_point.row {
        position.column < range.end_point.column
    } else {
        false
    }
}

fn find_tree_node(file_path: &Path, shader_content: &str, node : Node, point: Point) -> ShaderSymbolList {
    let mut filter_child = |child_node: Node| -> ShaderSymbolList {
        if is_in_range(child_node.range(), point) {
            find_tree_node(file_path, shader_content, child_node, point)
        } else {
            ShaderSymbolList::default()
        }
    };
    if is_in_range(node.range(), point) {
        let symbols_in_range = match node.kind() {
            // Looking for an identifier (variable or function call)
            "function_definition" => parse_function(file_path, node, shader_content, &mut filter_child),
            "declaration" => parse_variable(file_path, node, shader_content, &mut filter_child),
            "struct_specifier" => parse_struct(file_path, node, shader_content, &mut filter_child),
            _ => {
                let mut symbols = ShaderSymbolList::default();
                for child in node.children(&mut node.walk()) {
                    if is_in_range(node.range(), point) {
                        symbols.append(find_tree_node(file_path, shader_content, child, point));
                    }
                }
                symbols
            }
        };
        symbols_in_range
    } else {
        ShaderSymbolList::default()
    }
}*/

pub fn find_symbol_at_position(path: &Path, content: &String, tree: Tree, position: ShaderPosition) -> Option<ShaderSymbol> {
    assert!(path == position.file_path);
    let symbols = parse_tree(path, content, tree);
    let mut symbols_in_range : Vec<ShaderSymbol> = symbols.list_all().into_iter().filter(|e| match &e.range {
        Some(range) => range.contain(&position),
        None => false
    }).collect();
    symbols_in_range.sort_by(|a, b| a.range.as_ref().unwrap().start.cmp(&b.range.as_ref().unwrap().start));
    symbols_in_range.last().map(|e| e.clone())
}

// For debugging tree.
fn recurse_parent(shader_content: &str, node : Node) -> String {
    match node.parent() {
        Some(parent) => format!("{}<{}", node.kind(), recurse_parent(shader_content, parent)),
        None => node.kind().into(),
    }
}
fn generate_debug_node(file_path: &Path, shader_content: &str, node : Node) -> Vec<ShaderSymbol> {
    let name : String = get_name(shader_content, node).into();
    let mut shader_symbols = vec![ShaderSymbol {
        label: name.clone(), 
        description: format!("Kind: {}\n\nName: {}\n\n Hierarchy: {}", node.kind(), name, recurse_parent(shader_content, node)), 
        version: "".into(), 
        stages: vec![], 
        link: None, 
        members: None, 
        signature: None,
        ty: None, 
        range: Some(ShaderRange::new(
            ShaderPosition::new(file_path.into(), node.start_position().row as u32, node.start_position().column as u32), 
            ShaderPosition::new(file_path.into(), node.end_position().row as u32, node.end_position().column as u32)
        )), 
        scope_stack: None 
    }];
    for child in node.children(&mut node.walk()) {
        shader_symbols.extend(generate_debug_node(file_path, shader_content, child))
    }
    shader_symbols
}
#[allow(dead_code)] 
pub fn generate_debug_tree(path: &Path, content: &String, tree: Tree) -> ShaderSymbolList {
    let root_node = tree.root_node();
    ShaderSymbolList {
        variables: generate_debug_node(path, content, root_node),
        ..Default::default()
    }    
}