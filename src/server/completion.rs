use std::ffi::OsStr;

use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, MarkupContent, Position, Url,
};

use crate::{
    server::ServerLanguage,
    shaders::{
        shader::ShadingLanguage,
        shader_error::ValidatorError,
        symbols::symbols::{ShaderPosition, ShaderSymbol, ShaderSymbolData, ShaderSymbolType},
    },
};

use super::hover::get_word_range_at_position;

impl ServerLanguage {
    pub fn recolt_completion(
        &self,
        uri: &Url,
        shading_language: ShadingLanguage,
        shader_source: String,
        position: Position,
        trigger_character: Option<String>,
    ) -> Result<Vec<CompletionItem>, ValidatorError> {
        let file_path = uri
            .to_file_path()
            .expect(format!("Failed to convert {} to a valid path.", uri).as_str());
        let validation_params = self.config.into_validation_params();
        let symbol_provider = self.get_symbol_provider(shading_language);
        let symbol_list = symbol_provider.get_all_symbols_in_scope(
            &shader_source,
            &file_path,
            &validation_params,
            Some(ShaderPosition {
                file_path: file_path.clone(),
                line: position.line as u32,
                pos: position.character as u32,
            }),
        );
        match trigger_character {
            Some(_) => {
                // Find owning scope.
                // TODO:
                match get_word_range_at_position(
                    &shader_source,
                    Position {
                        line: position.line,
                        character: if position.character == 0 {
                            0
                        } else {
                            position.character - 1
                        },
                    },
                ) {
                    Some((word, _range)) => {
                        match symbol_list.find_variable_symbol(&word) {
                            Some(symbol) => {
                                if let ShaderSymbolData::Variables { ty } = &symbol.data {
                                    // Check type & find values in scope.
                                    match symbol_list.find_type_symbol(ty) {
                                        Some(ty_symbol) => {
                                            // We read the file and look for members
                                            if let ShaderSymbolData::Struct { members, methods } =
                                                ty_symbol.data
                                            {
                                                let members_converted: Vec<CompletionItem> =
                                                    members
                                                        .into_iter()
                                                        .map(|s| {
                                                            convert_completion_item(
                                                                shading_language,
                                                                s.as_symbol(),
                                                                CompletionItemKind::VARIABLE,
                                                            )
                                                        })
                                                        .collect();
                                                let methods_converted: Vec<CompletionItem> =
                                                    methods
                                                        .into_iter()
                                                        .map(|s| {
                                                            convert_completion_item(
                                                                shading_language,
                                                                s.as_symbol(),
                                                                CompletionItemKind::FUNCTION,
                                                            )
                                                        })
                                                        .collect();
                                                Ok(members_converted
                                                    .into_iter()
                                                    .chain(methods_converted.into_iter())
                                                    .collect())
                                            } else {
                                                Ok(vec![])
                                            }
                                        }
                                        None => Ok(vec![]),
                                    }
                                } else {
                                    Ok(vec![])
                                }
                            }
                            None => Ok(vec![]),
                        }
                    }
                    None => Ok(vec![]),
                }
            }
            None => Ok(symbol_list
                .into_iter()
                .map(|(symbol_list, ty)| {
                    symbol_list
                        .into_iter()
                        .map(|s| {
                            convert_completion_item(
                                shading_language,
                                s,
                                match ty {
                                    ShaderSymbolType::Types => CompletionItemKind::TYPE_PARAMETER,
                                    ShaderSymbolType::Constants => CompletionItemKind::CONSTANT,
                                    ShaderSymbolType::Variables => CompletionItemKind::VARIABLE,
                                    ShaderSymbolType::Functions => CompletionItemKind::FUNCTION,
                                    ShaderSymbolType::Keyword => CompletionItemKind::KEYWORD,
                                },
                            )
                        })
                        .collect()
                })
                .collect::<Vec<Vec<CompletionItem>>>()
                .concat()),
        }
    }
}

fn convert_completion_item(
    shading_language: ShadingLanguage,
    shader_symbol: ShaderSymbol,
    completion_kind: CompletionItemKind,
) -> CompletionItem {
    let doc_link = if let Some(link) = &shader_symbol.link {
        if !link.is_empty() {
            format!("\n[Online documentation]({})", link)
        } else {
            "".to_string()
        }
    } else {
        "".to_string()
    };
    let doc_signature = if let ShaderSymbolData::Functions { signatures } = &shader_symbol.data {
        // TODO: should not hide variants
        let parameters = signatures[0]
            .parameters
            .iter()
            .map(|p| format!("- `{} {}` {}", p.ty, p.label, p.description))
            .collect::<Vec<String>>();
        let parameters_markdown = if parameters.is_empty() {
            "".into()
        } else {
            format!("**Parameters:**\n\n{}", parameters.join("\n\n"))
        };
        format!(
            "\n**Return type:**\n\n`{}` {}\n\n{}",
            signatures[0].returnType, signatures[0].description, parameters_markdown
        )
    } else {
        "".to_string()
    };
    let position = if let Some(position) = &shader_symbol.position {
        format!(
            "{}:{}:{}",
            position
                .file_path
                .file_name()
                .unwrap_or(OsStr::new("file"))
                .to_string_lossy(),
            position.line,
            position.pos
        )
    } else {
        "".to_string()
    };
    let shading_language = shading_language.to_string();
    let description = {
        let mut description = shader_symbol.description.clone();
        let max_len = 500;
        if description.len() > max_len {
            description.truncate(max_len);
            description.push_str("...");
        }
        description
    };

    let signature = shader_symbol.format();
    CompletionItem {
        kind: Some(completion_kind),
        label: shader_symbol.label.clone(),
        detail: None,
        label_details: Some(CompletionItemLabelDetails {
            detail: None,
            description: if let ShaderSymbolData::Functions { signatures } = &shader_symbol.data {
                Some(if signatures.len() > 1 {
                    format!("{} (+ {})", signatures[0].format(shader_symbol.label.as_str()), signatures.len() - 1)
                } else {
                    signatures[0].format(shader_symbol.label.as_str())
                })
            } else {
                None
            },
        }),
        filter_text: Some(shader_symbol.label.clone()),
        documentation: Some(lsp_types::Documentation::MarkupContent(MarkupContent {
            kind: lsp_types::MarkupKind::Markdown,
            value: format!("```{shading_language}\n{signature}\n```\n{description}\n\n{doc_signature}\n\n{position}\n{doc_link}"),
        })),
        ..Default::default()
    }
}
