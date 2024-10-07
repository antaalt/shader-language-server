use std::path::Path;

use super::{parser::{get_name, SymbolTreeParser}, symbols::{ShaderParameter, ShaderRange, ShaderScope, ShaderSignature, ShaderSymbol, ShaderSymbolData, ShaderSymbolList, SymbolFilter}};

pub(super) struct HlslFunctionTreeParser {}

impl SymbolTreeParser for HlslFunctionTreeParser {
    fn get_query(&self) -> &str {
        // Ensure we dont pick struct_specifier
        r#"(function_definition
            type: [
                (type_identifier) @function.return
                (primitive_type) @function.return
            ]
            function_declarator: (function_declarator
                declarator: (identifier) @function.label
                parameter_list: (parameter_list 
                    ((parameter_declaration
                        type_identifier: (_) @function.param.type
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
