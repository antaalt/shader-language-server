use std::path::Path;

use crate::shaders::{include::IncludeHandler, shader::ShadingLanguage, symbols::symbols::ShaderMember};

use super::{
    parser::{get_name, SymbolTreeParser},
    symbols::{
        ShaderLabelSignature, ShaderMethod, ShaderParameter, ShaderPosition, ShaderRange, ShaderScope, ShaderSignature, ShaderSymbol, ShaderSymbolData, ShaderSymbolList
    },
};

pub(super) struct HlslIncludeTreeParser {}

impl SymbolTreeParser for HlslIncludeTreeParser {
    fn get_query(&self) -> String {
        // TODO: string_content unsupported on tree_sitter 0.20.9
        /*r#"(preproc_include
            (#include)
            path: (string_literal
                (string_content) @include
            )
        )"#*/
        r#"(preproc_include
            (#include)
            path: (string_literal) @include
        )"#.into()
    }
    fn process_match(
        &self,
        matches: tree_sitter::QueryMatch,
        file_path: &Path,
        shader_content: &str,
        _scopes: &Vec<ShaderScope>,
        symbols: &mut ShaderSymbolList,
    ) {
        let include_node = matches.captures[0].node;
        let range = ShaderRange::from_range(include_node.range(), file_path.into());
        let mut include_handler = IncludeHandler::new(file_path, vec![]); // TODO: pass includes aswell ?
        let relative_path = get_name(shader_content, include_node);
        let relative_path = &relative_path[1..relative_path.len() - 1]; // TODO: use string_content instead

        // Only add symbol if path can be resolved.
        match include_handler.search_path_in_includes(Path::new(relative_path)) {
            Some(absolute_path) => {
                symbols.functions.push(ShaderSymbol {
                    label: relative_path.into(),
                    description: format!("Including file {}", absolute_path.display()),
                    version: "".into(),
                    stages: vec![],
                    link: None,
                    data: ShaderSymbolData::Link {
                        target: ShaderPosition::new(absolute_path, 0, 0),
                    },
                    range: Some(range),
                    scope_stack: None, // No scope for include
                });
            }
            None => {}
        }
    }
}
pub(super) struct HlslDefineTreeParser {}

impl SymbolTreeParser for HlslDefineTreeParser {
    fn get_query(&self) -> String {
        r#"(preproc_def
            (#define)
            name: (identifier) @define.label
            value: (preproc_arg)? @define.value
        )"#.into()
    }
    fn process_match(
        &self,
        matches: tree_sitter::QueryMatch,
        file_path: &Path,
        shader_content: &str,
        _scopes: &Vec<ShaderScope>,
        symbols: &mut ShaderSymbolList,
    ) {
        let identifier_node = matches.captures[0].node;
        let range = ShaderRange::from_range(identifier_node.range(), file_path.into());
        let value = if matches.captures.len() > 1 {
            Some(get_name(shader_content, matches.captures[1].node).trim())
        } else {
            None
        };
        symbols.functions.push(ShaderSymbol {
            label: get_name(shader_content, identifier_node).into(),
            description: match value {
                Some(value) => format!(
                    "Preprocessor macro. Expanding to \n```{}\n{}\n```",
                    ShadingLanguage::Hlsl.to_string(),
                    value
                ),
                None => format!("Preprocessor macro."),
            },
            version: "".into(),
            stages: vec![],
            link: None,
            data: ShaderSymbolData::Constants {
                ty: "#define".into(),
                qualifier: "".into(),
                value: match value {
                    Some(value) => value.into(),
                    None => "".into(),
                },
            },
            range: Some(range),
            scope_stack: None, // No scope for include
        });
    }
}
pub(super) struct HlslFunctionTreeParser {
    pub is_field: bool
}

impl SymbolTreeParser for HlslFunctionTreeParser {
    fn get_query(&self) -> String {
        let field_prestring = if self.is_field {
            "field_" 
        } else {
            ""
        };
        format!(r#"(function_definition
            type: (_) @function.return
            declarator: (function_declarator
                declarator: ({}identifier) @function.label
                parameters: (parameter_list 
                    ((parameter_declaration
                        type: (_) @function.param.type
                        declarator: (_) @function.param.decl
                    )(",")?)*
                )
            )
            body: (compound_statement) @function.scope
        )"#, field_prestring) // compound_statement is function scope.
            /*(semantics
                (identifier) @function.param.semantic
            )?*/
    }
    fn process_match(
        &self,
        matches: tree_sitter::QueryMatch,
        file_path: &Path,
        shader_content: &str,
        scopes: &Vec<ShaderScope>,
        symbols: &mut ShaderSymbolList,
    ) {
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

pub(super) struct HlslStructTreeParser {
    var_parser: HlslVariableTreeParser,
    var_query: tree_sitter::Query,
    func_parser: HlslFunctionTreeParser,
    func_query: tree_sitter::Query,
}
impl HlslStructTreeParser {
    pub fn new() -> Self {
        // Cache for perf.
        let lang = tree_sitter_hlsl::language();
        let func_parser = HlslFunctionTreeParser{ is_field: true };
        let var_parser = HlslVariableTreeParser{ is_field: true };
        let var_query = var_parser.get_query();
        let func_query = func_parser.get_query();
        Self {
            var_parser,
            var_query: tree_sitter::Query::new(lang.clone(), var_query.as_str()).unwrap(),
            func_parser,
            func_query: tree_sitter::Query::new(lang.clone(), func_query.as_str()).unwrap(),
        }
    }
}
impl SymbolTreeParser for HlslStructTreeParser {
    fn get_query(&self) -> String {
        r#"(struct_specifier
            name: (type_identifier) @struct.type
            body: (field_declaration_list) @struct.content
        )"#.into()
    }
    fn process_match(
        &self,
        matches: tree_sitter::QueryMatch,
        file_path: &Path,
        shader_content: &str,
        scopes: &Vec<ShaderScope>,
        symbols: &mut ShaderSymbolList,
    ) {
        let label_node = matches.captures[0].node;
        let range = ShaderRange::from_range(label_node.range(), file_path.into());
        let scope_stack = self.compute_scope_stack(&scopes, &range);
                
        // QUERY INNER METHODS
        let mut query_cursor = tree_sitter::QueryCursor::new();
        let methods = query_cursor.matches(
            &self.func_query,
            matches.captures[1].node,
            shader_content.as_bytes(),
        ).map(|matches| {
            let mut symbols = ShaderSymbolList::default();
            self.func_parser.process_match(matches, file_path, shader_content, scopes, &mut symbols);
            symbols.functions.iter().map(|f| ShaderMethod {
                label: f.label.clone(),
                signature: if let ShaderSymbolData::Functions { signatures } = &f.data {
                    signatures[0].clone()
                } else {
                    panic!("Wowo");
                },
            }).collect::<Vec<ShaderMethod>>()
        }).collect::<Vec<Vec<ShaderMethod>>>().concat();
        
        // QUERY INNER MEMBERS
        let mut query_cursor = tree_sitter::QueryCursor::new();
        let members = query_cursor.matches(
            &self.var_query,
            matches.captures[1].node,
            shader_content.as_bytes(),
        ).map(|matches| {
            let mut symbols = ShaderSymbolList::default();
            self.var_parser.process_match(matches, file_path, shader_content, scopes, &mut symbols);
            symbols.variables.iter().map(|f| ShaderMember {
                label: f.label.clone(),
                ty: if let ShaderSymbolData::Variables { ty } = &f.data {
                    ty.clone()
                } else {
                    panic!("Invalid type");
                },
                description: "".into(),
            }).collect::<Vec<ShaderMember>>()
        }).collect::<Vec<Vec<ShaderMember>>>().concat();
        // Should run function & variable capture within the struct bounds instead ?
        symbols.types.push(ShaderSymbol {
            label: get_name(shader_content, matches.captures[0].node).into(),
            description: "".into(),
            version: "".into(),
            stages: vec![],
            link: None,
            data: ShaderSymbolData::Struct {
                members: members,
                methods: methods,
            },
            range: Some(range),
            scope_stack: Some(scope_stack),
        });
    }
}

pub(super) struct HlslVariableTreeParser {
    pub is_field: bool
}

impl SymbolTreeParser for HlslVariableTreeParser {
    fn get_query(&self) -> String {
        let field_prestring = if self.is_field {
            "field_" 
        } else {
            ""
        };
        format!(r#"({}declaration
            type: (_) @variable.type
            declarator: [(init_declarator
                declarator: (identifier) @variable.label
                value: (_) @variable.value
            ) 
            ({}identifier) @variable.label
            ]
        )"#, field_prestring, field_prestring)
    }
    fn process_match(
        &self,
        matches: tree_sitter::QueryMatch,
        file_path: &Path,
        shader_content: &str,
        scopes: &Vec<ShaderScope>,
        symbols: &mut ShaderSymbolList,
    ) {
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
