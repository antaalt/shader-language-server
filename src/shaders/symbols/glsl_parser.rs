// TODO: rename glsl.rs to glsl_filter.rs

use std::path::Path;

use super::{parser::{get_name, SymbolTreeParser}, symbols::{ShaderParameter, ShaderRange, ShaderScope, ShaderSignature, ShaderSymbol, ShaderSymbolData, ShaderSymbolList}};

pub(super) struct GlslFunctionTreeParser {}

impl SymbolTreeParser for GlslFunctionTreeParser {
    fn get_query(&self) -> &str {
        // could use include_str! for scm file.
        r#"(function_definition
            type: [
                (type_identifier) @function.return
                (primitive_type) @function.return
            ]
            declarator: (function_declarator
                declarator: (identifier) @function.label
                parameters: (parameter_list 
                    ((parameter_declaration
                        type: (_) @function.param.type
                        declarator: (_) @function.param.decl
                    )(",")?)*
                )
            )
            body: (compound_statement) @function.scope
            )"# // compound_statement is function scope.
    }
    fn process_match(&self, matches: tree_sitter::QueryMatch, file_path: &Path, shader_content: &str, scopes: &Vec<ShaderScope>, symbols: &mut ShaderSymbolList) {
        let label_node = matches.captures[1].node;
        let range = ShaderRange::from_range(label_node.range(), file_path.into());
        let scope_stack = self.compute_scope_stack(scopes, &range);
        // Query internal scopes variables
        /*let scope_node = matche.captures[matche.captures.len() - 1].node;
        let content_scope_stack = {
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
            label: get_name(shader_content, matches.captures[1].node).into(),
            description: "".into(),
            version: "".into(),
            stages: vec![],
            link: None,
            data: ShaderSymbolData::Functions {
                signatures: vec![ShaderSignature {
                    returnType: get_name(shader_content, matches.captures[0].node).into(),
                    description: "".into(),
                    parameters: matches.captures[2..matches.captures.len() - 1]
                        .chunks(2)
                        .map(|w| ShaderParameter {
                            ty: get_name(shader_content, w[0].node).into(),
                            label: get_name(shader_content, w[1].node).into(),
                            description: "".into(),
                        })
                        .collect::<Vec<ShaderParameter>>(),
                }],
            },
            range: Some(range),
            scope_stack: Some(scope_stack), // In GLSL, all function are global scope.
        });
    }
}

pub(super) struct GlslStructTreeParser {}

impl SymbolTreeParser for GlslStructTreeParser {
    fn get_query(&self) -> &str {
        r#"(struct_specifier
            name: (type_identifier) @struct.type
            body: (field_declaration_list
                (field_declaration 
                    type: (_) @struct.param.type
                    declarator: (_) @struct.param.decl
                )+
            )
        )"#
    }
    fn process_match(&self, matches: tree_sitter::QueryMatch, file_path: &Path, shader_content: &str, scopes: &Vec<ShaderScope>, symbols: &mut ShaderSymbolList) {
        let label_node = matches.captures[0].node;
        let range = ShaderRange::from_range(label_node.range(), file_path.into());
        let scope_stack = self.compute_scope_stack(&scopes, &range);
        symbols.types.push(ShaderSymbol {
            label: get_name(shader_content, matches.captures[0].node).into(),
            description: "".into(),
            version: "".into(),
            stages: vec![],
            link: None,
            data: ShaderSymbolData::Struct {
                members: matches.captures[1..]
                    .chunks(2)
                    .map(|w| ShaderParameter {
                        ty: get_name(shader_content, w[0].node).into(),
                        label: get_name(shader_content, w[1].node).into(),
                        description: "".into(),
                    })
                    .collect::<Vec<ShaderParameter>>(),
                methods: vec![],
            },
            range: Some(range),
            scope_stack: Some(scope_stack),
        });
    }
}
pub(super) struct GlslVariableTreeParser {}

impl SymbolTreeParser for GlslVariableTreeParser {
    fn get_query(&self) -> &str {
        r#"(declaration
            type: [
                (type_identifier) @variable.type
                (primitive_type) @variable.type
            ]
            declarator: [(init_declarator
                declarator: (identifier) @variable.label
                value: (_) @variable.value
            ) 
            (identifier) @variable.label
            ]
        )"#
    }
    fn process_match(&self, matches: tree_sitter::QueryMatch, file_path: &Path, shader_content: &str, scopes: &Vec<ShaderScope>, symbols: &mut ShaderSymbolList) {
        let label_node = matches.captures[1].node;
        let range = ShaderRange::from_range(label_node.range(), file_path.into());
        let scope_stack = self.compute_scope_stack(&scopes, &range);
        // Check if its parameter or struct element.
        let _type_qualifier = get_name(shader_content, matches.captures[0].node);
        // TODO: handle values & qualifiers..
        //let _value = get_name(shader_content, matche.captures[2].node);
        symbols.variables.push(ShaderSymbol {
            label: get_name(shader_content, matches.captures[1].node).into(),
            description: "".into(),
            version: "".into(),
            stages: vec![],
            link: None,
            data: ShaderSymbolData::Variables {
                ty: get_name(shader_content, matches.captures[0].node).into(),
            },
            range: Some(range),
            scope_stack: Some(scope_stack),
        });
    }
}