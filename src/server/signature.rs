use std::io::{BufRead, BufReader};

use log::debug;
use lsp_types::{
    MarkupContent, ParameterInformation, ParameterLabel, Position, SignatureHelp,
    SignatureInformation, Url,
};
use regex::Regex;

use crate::{
    server::ServerLanguage,
    shaders::{
        shader::ShadingLanguage,
        shader_error::ValidatorError,
        symbols::symbols::{ShaderPosition, ShaderSymbol, ShaderSymbolData},
    },
};

impl ServerLanguage {
    pub fn recolt_signature(
        &mut self,
        uri: &Url,
        shading_language: ShadingLanguage,
        content: String,
        position: Position,
    ) -> Result<Option<SignatureHelp>, ValidatorError> {
        let function_parameter = get_function_parameter_at_position(&content, position);
        debug!("Found requested func name {:?}", function_parameter);

        let file_path = uri
            .to_file_path()
            .expect(format!("Failed to convert {} to a valid path.", uri).as_str());
        let validation_params = self.config.into_validation_params();

        let symbol_provider = self.get_symbol_provider(shading_language);
        let completion = symbol_provider.get_all_symbols_in_scope(
            &content,
            &file_path,
            &validation_params,
            Some(ShaderPosition {
                file_path: file_path.clone(),
                line: position.line as u32,
                pos: position.character as u32,
            }),
        );
        let (shader_symbols, parameter_index): (Vec<&ShaderSymbol>, u32) =
            if let (Some(function), Some(parameter_index)) = function_parameter {
                (
                    completion
                        .functions
                        .iter()
                        .filter(|shader_symbol| shader_symbol.label == function)
                        .collect(),
                    parameter_index,
                )
            } else {
                (Vec::new(), 0)
            };
        let signatures: Vec<SignatureInformation> = shader_symbols
            .iter()
            .filter_map(|shader_symbol| {
                if let ShaderSymbolData::Functions { signatures } = &shader_symbol.data {
                    Some(signatures.iter().map(|signature| SignatureInformation {
                        label: signature.format(shader_symbol.label.as_str()),
                        documentation: Some(lsp_types::Documentation::MarkupContent(
                            MarkupContent {
                                kind: lsp_types::MarkupKind::Markdown,
                                value: shader_symbol.description.clone(),
                            },
                        )),
                        parameters: Some(
                            signature
                                .parameters
                                .iter()
                                .map(|e| ParameterInformation {
                                    label: ParameterLabel::Simple(e.label.clone()),
                                    documentation: Some(lsp_types::Documentation::MarkupContent(
                                        MarkupContent {
                                            kind: lsp_types::MarkupKind::Markdown,
                                            value: e.description.clone(),
                                        },
                                    )),
                                })
                                .collect(),
                        ),
                        active_parameter: None,
                    }).collect::<Vec<SignatureInformation>>())
                } else {
                    None
                }
            })
            .collect::<Vec<Vec<SignatureInformation>>>().concat();
        if signatures.is_empty() {
            debug!("No signature for symbol {:?} found", shader_symbols);
            Ok(None)
        } else {
            Ok(Some(SignatureHelp {
                signatures: signatures,
                active_signature: None,
                active_parameter: Some(parameter_index), // TODO: check out of bounds.
            }))
        }
    }
}
fn get_function_parameter_at_position(
    shader: &String,
    position: Position,
) -> (Option<String>, Option<u32>) {
    let reader = BufReader::new(shader.as_bytes());
    let line = reader
        .lines()
        .nth(position.line as usize)
        .expect("Text position is out of bounds")
        .expect("Could not read line");
    // Check this regex is working for all lang.
    let regex =
        Regex::new("\\b([a-zA-Z_][a-zA-Z0-9_]*)(\\(.*?)(\\))").expect("Failed to init regex");
    for capture in regex.captures_iter(line.as_str()) {
        let file_name = capture.get(1).expect("Failed to get function name");
        let parenthesis = capture.get(2).expect("Failed to get paranthesis");
        let parameter_index = if position.character >= parenthesis.start() as u32
            && position.character <= parenthesis.end() as u32
        {
            let parameters = line[parenthesis.start()..parenthesis.end()].to_string();
            let parameters = parameters.split(',');
            let pos_in_parameters = position.character as usize - parenthesis.start();
            // Compute parameter index
            let mut parameter_index = 0;
            let mut parameter_offset = 0;
            for parameter in parameters {
                parameter_offset += parameter.len() + 1; // Add 1 for removed comma
                if parameter_offset > pos_in_parameters {
                    break;
                }
                parameter_index += 1;
            }
            Some(parameter_index)
        } else {
            None
        };
        if position.character >= file_name.start() as u32
            && position.character <= parenthesis.end() as u32
        {
            return (
                Some(line[file_name.start()..file_name.end()].to_string()),
                parameter_index,
            );
        }
    }
    // No signature
    (None, None)
}
