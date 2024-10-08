use std::path::Path;

use regex::{Captures, Regex};

use crate::shaders::shader::ShaderStage;

use super::symbols::ShaderMember;
use super::symbols::{
    ShaderParameter, ShaderPosition, ShaderScope, ShaderSignature, ShaderSymbol, ShaderSymbolData,
    ShaderSymbolList, ShaderSymbolType, SymbolFilter, SymbolParser, SymbolProvider,
};

pub(super) struct HlslFunctionParser {}
impl SymbolParser for HlslFunctionParser {
    fn get_symbol_type(&self) -> ShaderSymbolType {
        ShaderSymbolType::Functions
    }
    fn get_capture_regex(&self) -> Option<Regex> {
        Some(
            Regex::new("\\b([\\w_]*)\\s+([\\w_-]*)[\\s]*\\(([\\s\\w\\,\\-\\[\\]]*)\\)[\\s]*\\{")
                .unwrap(),
        )
    }

    fn parse_capture(
        &self,
        capture: regex::Captures,
        shader_content: &String,
        path: &Path,
        scopes: &Vec<ShaderScope>,
    ) -> ShaderSymbol {
        let signature = capture.get(2).unwrap();
        let return_type = capture.get(1).unwrap().as_str();
        let function = capture.get(2).unwrap().as_str();
        let parameters: Vec<&str> = match capture.get(3) {
            Some(all_parameters) => {
                if all_parameters.is_empty() {
                    Vec::new()
                } else {
                    all_parameters.as_str().split(',').collect()
                }
            }
            None => Vec::new(),
        };
        let position = ShaderPosition::from_pos(&shader_content, signature.start(), path);

        ShaderSymbol {
            label: function.into(),
            description: "".into(),
            version: "".into(),
            stages: Vec::new(),
            link: None,
            data: ShaderSymbolData::Functions {
                signatures: vec![ShaderSignature {
                    returnType: return_type.to_string(),
                    description: "".into(),
                    parameters: parameters
                        .iter()
                        .map(|parameter| {
                            let values: Vec<&str> = parameter.split_whitespace().collect();
                            ShaderParameter {
                                ty: (*values.first().unwrap_or(&"void")).into(),
                                label: (*values.last().unwrap_or(&"type")).into(),
                                description: "".into(),
                            }
                        })
                        .collect(),
                }],
            },
            position: Some(position.clone()),
            scope_stack: Some(SymbolProvider::compute_scope_stack(&position, scopes)),
        }
    }
}
pub(super) struct HlslStructParser {}
impl SymbolParser for HlslStructParser {
    fn get_symbol_type(&self) -> ShaderSymbolType {
        ShaderSymbolType::Types
    }
    fn get_capture_regex(&self) -> Option<Regex> {
        Some(Regex::new("\\bstruct\\s+([\\w_-]+)\\s*\\{([^}]*)\\}").unwrap())
    }

    fn parse_capture(
        &self,
        capture: Captures,
        shader_content: &String,
        path: &Path,
        scopes: &Vec<ShaderScope>,
    ) -> ShaderSymbol {
        let name = capture.get(1).unwrap();
        let struct_content = capture.get(2).unwrap().as_str();

        // Parse members
        let member_regex = Regex::new("\\b([\\w_]*)\\s+([\\w_-]*)\\s*[;=][^=]").unwrap();
        let members: Vec<ShaderMember> = member_regex
            .captures_iter(struct_content)
            .map(|member_capture| {
                let ty = member_capture.get(1).unwrap();
                let name = member_capture.get(2).unwrap();

                let _position = ShaderPosition::from_pos(&shader_content, name.start(), path);
                ShaderMember {
                    label: name.as_str().into(),
                    ty: ty.as_str().into(),
                    description: "".into(),
                }
            })
            .collect();

        let position = ShaderPosition::from_pos(&shader_content, name.start(), path);
        ShaderSymbol {
            label: name.as_str().into(),
            description: "".into(),
            version: "".into(),
            stages: Vec::new(),
            link: None,
            data: ShaderSymbolData::Struct {
                members: members,
                methods: vec![],
            },
            position: Some(position.clone()),
            scope_stack: Some(SymbolProvider::compute_scope_stack(&position, scopes)),
        }
    }
}
pub(super) struct HlslMacroParser {}
impl SymbolParser for HlslMacroParser {
    fn get_symbol_type(&self) -> ShaderSymbolType {
        ShaderSymbolType::Constants
    }
    fn get_capture_regex(&self) -> Option<Regex> {
        Some(Regex::new("\\#define\\s+([\\w\\-]+)").unwrap())
    }

    fn parse_capture(
        &self,
        capture: Captures,
        shader_content: &String,
        path: &Path,
        scopes: &Vec<ShaderScope>,
    ) -> ShaderSymbol {
        let value = capture.get(1).unwrap();

        let position = ShaderPosition::from_pos(&shader_content, value.start(), path);
        ShaderSymbol {
            label: value.as_str().into(),
            description: "preprocessor macro".into(),
            version: "".into(),
            stages: Vec::new(),
            link: None,
            data: ShaderSymbolData::Constants {
                ty: "".into(),
                qualifier: "".into(),
                value: "".into(),
            },
            position: Some(position.clone()),
            scope_stack: Some(SymbolProvider::compute_scope_stack(&position, scopes)),
        }
    }
}
pub(super) struct HlslVariableParser {}
impl SymbolParser for HlslVariableParser {
    fn get_symbol_type(&self) -> ShaderSymbolType {
        ShaderSymbolType::Variables
    }

    fn get_capture_regex(&self) -> Option<Regex> {
        Some(Regex::new("\\b([\\w_]*)\\s+([\\w_-]*)\\s*[;=][^=]").unwrap())
    }

    fn parse_capture(
        &self,
        capture: Captures,
        shader_content: &String,
        path: &Path,
        scopes: &Vec<ShaderScope>,
    ) -> ShaderSymbol {
        let ty = capture.get(1).unwrap();
        let name = capture.get(2).unwrap();

        let position = ShaderPosition::from_pos(&shader_content, name.start(), path);
        ShaderSymbol {
            label: name.as_str().into(),
            description: "".into(),
            version: "".into(),
            stages: Vec::new(),
            link: None,
            data: ShaderSymbolData::Variables {
                ty: ty.as_str().into(),
            },
            position: Some(position.clone()),
            scope_stack: Some(SymbolProvider::compute_scope_stack(&position, scopes)),
        }
    }
}
pub struct HlslVersionFilter {}

impl SymbolFilter for HlslVersionFilter {
    fn filter_symbols(&self, _shader_symbols: &mut ShaderSymbolList, _file_name: &String) {
        // TODO: filter version
    }
}
pub struct HlslStageFilter {}

impl SymbolFilter for HlslStageFilter {
    fn filter_symbols(&self, shader_symbols: &mut ShaderSymbolList, file_name: &String) {
        match ShaderStage::from_file_name(file_name) {
            Some(shader_stage) => {
                *shader_symbols = ShaderSymbolList {
                    types: shader_symbols
                        .types
                        .drain(..)
                        .filter(|value| {
                            value.stages.contains(&shader_stage) || value.stages.is_empty()
                        })
                        .collect(),
                    constants: shader_symbols
                        .constants
                        .drain(..)
                        .filter(|value| {
                            value.stages.contains(&shader_stage) || value.stages.is_empty()
                        })
                        .collect(),
                    variables: shader_symbols
                        .variables
                        .drain(..)
                        .filter(|value| {
                            value.stages.contains(&shader_stage) || value.stages.is_empty()
                        })
                        .collect(),
                    functions: shader_symbols
                        .functions
                        .drain(..)
                        .filter(|value| {
                            value.stages.contains(&shader_stage) || value.stages.is_empty()
                        })
                        .collect(),
                    keywords: shader_symbols
                        .keywords
                        .drain(..)
                        .filter(|value| {
                            value.stages.contains(&shader_stage) || value.stages.is_empty()
                        })
                        .collect(),
                }
            }
            None => {
                // No filtering
            }
        }
    }
}
