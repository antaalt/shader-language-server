use std::path::Path;

use regex::{Captures, Regex};

use super::symbols::{
    DeclarationParser, ShaderParameter, ShaderPosition, ShaderScope, ShaderSignature, ShaderSymbol,
    ShaderSymbolType, SymbolProvider,
};

pub(super) struct GlslFunctionParser {}
impl DeclarationParser for GlslFunctionParser {
    fn get_symbol_type(&self) -> ShaderSymbolType {
        ShaderSymbolType::Functions
    }
    fn get_capture_regex(&self) -> Option<Regex> {
        Some(
            Regex::new("\\b([\\w_]*)\\s+([\\w_-]*)[\\s]*\\(([\\s\\w,-\\[\\]]*)\\)[\\s]*\\{")
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
            signature: Some(ShaderSignature {
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
            }),
            ty: None,
            position: Some(position.clone()),
            scope_stack: Some(SymbolProvider::compute_scope_stack(&position, scopes)),
        }
    }
}
pub(super) struct GlslStructParser {}
impl DeclarationParser for GlslStructParser {
    fn get_symbol_type(&self) -> ShaderSymbolType {
        ShaderSymbolType::Types
    }
    fn get_capture_regex(&self) -> Option<Regex> {
        Some(Regex::new("\\bstruct\\s+([\\w_-]+)\\s*\\{").unwrap())
    }

    fn parse_capture(
        &self,
        capture: Captures,
        shader_content: &String,
        path: &Path,
        scopes: &Vec<ShaderScope>,
    ) -> ShaderSymbol {
        let name = capture.get(1).unwrap();

        let position = ShaderPosition::from_pos(&shader_content, name.start(), path);
        ShaderSymbol {
            label: name.as_str().into(),
            description: "".into(),
            version: "".into(),
            stages: Vec::new(),
            link: None,
            signature: None,
            ty: None,
            position: Some(position.clone()),
            scope_stack: Some(SymbolProvider::compute_scope_stack(&position, scopes)),
        }
    }
}
pub(super) struct GlslMacroParser {}
impl DeclarationParser for GlslMacroParser {
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
            signature: None,
            ty: None,
            position: Some(position.clone()),
            scope_stack: Some(SymbolProvider::compute_scope_stack(&position, scopes)),
        }
    }
}
pub(super) struct GlslVariableParser {}
impl DeclarationParser for GlslVariableParser {
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
            signature: None,
            ty: Some(ty.as_str().into()),
            position: Some(position.clone()),
            scope_stack: Some(SymbolProvider::compute_scope_stack(&position, scopes)),
        }
    }
}
