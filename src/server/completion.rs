use std::{collections::HashMap, ffi::OsStr};

use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, MarkupContent, Position, Url,
};

use crate::{
    server::ServerLanguage,
    shaders::{
        shader::ShadingLanguage,
        shader_error::ValidatorError,
        symbols::symbols::{ShaderPosition, ShaderSymbol, ShaderSymbolType},
        validator::validator::ValidationParams,
    },
};

impl ServerLanguage {
    pub fn recolt_completion(
        &mut self,
        uri: &Url,
        shading_language: ShadingLanguage,
        shader_source: String,
        position: Position,
        trigger_character: Option<String>,
    ) -> Result<Vec<CompletionItem>, ValidatorError> {
        let file_path = uri
            .to_file_path()
            .expect(format!("Failed to convert {} to a valid path.", uri).as_str());
        let validation_params =
            ValidationParams::new(self.config.includes.clone(), self.config.defines.clone());
        let symbol_provider = self.get_symbol_provider(shading_language);
        let completion = symbol_provider.get_all_symbols_in_scope(
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
                match symbol_provider.get_symbol_at_position(
                    &shader_source,
                    &file_path,
                    &validation_params,
                    ShaderPosition {
                        file_path: file_path.clone(),
                        line: position.line,
                        pos: if position.character == 0 {
                            0
                        } else {
                            position.character - 1
                        },
                    },
                ) {
                    Some(symbol) => {
                        match &symbol.ty {
                            Some(ty) => {
                                // Check type & find values in scope.
                                match completion.find_type_symbol(ty) {
                                    Some(ty_symbol) => {
                                        // We read the file and look for members
                                        match ty_symbol.members {
                                            Some(members) => {
                                                let mut converted_members: Vec<CompletionItem> =
                                                    members
                                                        .members
                                                        .iter()
                                                        .map(|e| {
                                                            convert_completion_item(
                                                                shading_language,
                                                                ShaderSymbol {
                                                                    label: e.label.clone(),
                                                                    description: e
                                                                        .description
                                                                        .clone(),
                                                                    version: "".into(),
                                                                    stages: vec![],
                                                                    link: None,
                                                                    members: None,
                                                                    signature: None,
                                                                    ty: Some(e.ty.clone()),
                                                                    range: None,
                                                                    scope_stack: None,
                                                                },
                                                                CompletionItemKind::VARIABLE,
                                                                None,
                                                            )
                                                        })
                                                        .collect();
                                                let converted_methods: Vec<CompletionItem> =
                                                    members
                                                        .methods
                                                        .iter()
                                                        .map(|e| {
                                                            convert_completion_item(
                                                                shading_language,
                                                                ShaderSymbol {
                                                                    label: e.label.clone(),
                                                                    description: e
                                                                        .description
                                                                        .clone(),
                                                                    version: "".into(),
                                                                    stages: vec![],
                                                                    link: None,
                                                                    members: None,
                                                                    signature: Some(
                                                                        e.signature.clone(),
                                                                    ),
                                                                    ty: None,
                                                                    range: None,
                                                                    scope_stack: None,
                                                                },
                                                                CompletionItemKind::VARIABLE,
                                                                None,
                                                            )
                                                        })
                                                        .collect();
                                                converted_members.extend(converted_methods);
                                                Ok(converted_members)
                                            }
                                            None => Ok(vec![]),
                                        }
                                    }
                                    None => Ok(vec![]),
                                }
                            }
                            None => Ok(vec![]),
                        }
                    }
                    None => Ok(vec![]),
                }
            }
            None => {
                let filter_symbols = |symbols: Vec<ShaderSymbol>| -> Vec<(ShaderSymbol, u32)> {
                    let mut set = HashMap::<String, (ShaderSymbol, u32)>::new();
                    for symbol in symbols {
                        match set.get_mut(&symbol.label) {
                            Some((_, count)) => *count += 1,
                            None => {
                                set.insert(symbol.label.clone(), (symbol, 1));
                            }
                        };
                    }
                    set.into_iter().map(|e| e.1).collect()
                };
                Ok(completion
                    .into_iter()
                    .map(|(symbol_list, ty)| {
                        filter_symbols(symbol_list)
                            .into_iter()
                            .map(|s| {
                                convert_completion_item(
                                    shading_language,
                                    s.0,
                                    match ty {
                                        ShaderSymbolType::Types => {
                                            CompletionItemKind::TYPE_PARAMETER
                                        }
                                        ShaderSymbolType::Constants => CompletionItemKind::CONSTANT,
                                        ShaderSymbolType::Variables => CompletionItemKind::VARIABLE,
                                        ShaderSymbolType::Functions => CompletionItemKind::FUNCTION,
                                        ShaderSymbolType::Keyword => CompletionItemKind::KEYWORD,
                                    },
                                    Some(s.1),
                                )
                            })
                            .collect()
                    })
                    .collect::<Vec<Vec<CompletionItem>>>()
                    .concat())
            }
        }
    }
}

fn convert_completion_item(
    shading_language: ShadingLanguage,
    shader_symbol: ShaderSymbol,
    completion_kind: CompletionItemKind,
    variant_count: Option<u32>,
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
    let doc_signature = if let Some(signature) = &shader_symbol.signature {
        let parameters = signature
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
            signature.returnType, signature.description, parameters_markdown
        )
    } else {
        "".to_string()
    };
    let position = if let Some(range) = &shader_symbol.range {
        format!(
            "{}:{}:{}",
            range
                .start
                .file_path
                .file_name()
                .unwrap_or(OsStr::new("file"))
                .to_string_lossy(),
            range.start.line,
            range.start.pos
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
            description: match &shader_symbol.signature {
                Some(sig) => Some(match variant_count {
                    Some(count) => if count > 1 {
                        format!("{} (+ {})", sig.format(shader_symbol.label.as_str()), count - 1)
                    } else {
                        sig.format(shader_symbol.label.as_str())
                    },
                    None => sig.format(shader_symbol.label.as_str()),
                }),
                None => None,
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
