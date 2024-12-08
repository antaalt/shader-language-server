use std::{ffi::OsStr, rc::Rc};

use log::{error, warn};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, MarkupContent, Position, Url,
};

use crate::shaders::{
    shader::ShadingLanguage,
    symbols::symbols::{
        ShaderPosition, ShaderSymbol, ShaderSymbolData, ShaderSymbolType, SymbolError,
    },
};

use super::{ServerFileCacheHandle, ServerLanguageData};

impl ServerLanguageData {
    fn list_members_and_methods(&self, symbol: &ShaderSymbol) -> Vec<ShaderSymbol> {
        if let ShaderSymbolData::Struct { members, methods } = &symbol.data {
            let mut converted_members: Vec<ShaderSymbol> =
                members.iter().map(|e| e.as_symbol()).collect();
            let converted_methods: Vec<ShaderSymbol> =
                methods.iter().map(|e| e.as_symbol()).collect();
            converted_members.extend(converted_methods);
            converted_members
        } else {
            Vec::new()
        }
    }

    pub fn recolt_completion(
        &mut self,
        uri: &Url,
        cached_file: ServerFileCacheHandle,
        position: Position,
        trigger_character: Option<String>,
    ) -> Result<Vec<CompletionItem>, SymbolError> {
        let file_path = uri.to_file_path().unwrap();
        let symbol_list = self.get_all_symbols(Rc::clone(&cached_file));
        let cached_file = cached_file.borrow();
        let shader_position = ShaderPosition {
            file_path: file_path.clone(),
            line: position.line as u32,
            // TODO: -1 should be able to go up a line.
            pos: if position.character == 0 {
                0
            } else {
                position.character - 1
            },
        };
        let symbol_list = symbol_list.filter_scoped_symbol(shader_position.clone());
        match trigger_character {
            Some(_) => {
                match self.symbol_provider.get_word_chain_range_at_position(
                    &cached_file.symbol_tree,
                    shader_position.clone(),
                ) {
                    Ok(chain) => {
                        let mut chain_list = chain.iter().rev();
                        let mut current_symbol = match chain_list.next() {
                            Some(next_item) => match symbol_list.find_symbol(&next_item.0) {
                                Some(symbol) => {
                                    if let ShaderSymbolData::Variables { ty } = &symbol.data {
                                        match symbol_list.find_type_symbol(ty) {
                                            Some(ty_symbol) => ty_symbol,
                                            None => {
                                                warn!("Symbol type {} is not found.", ty);
                                                return Ok(vec![]);
                                            }
                                        }
                                    } else {
                                        error!("Not variable {:?}", symbol);
                                        return Ok(vec![]); // Nothing valid under cursor
                                    }
                                }
                                None => {
                                    error!("No symbol found for {}", next_item.0);
                                    return Ok(vec![]);
                                } // Nothing valid under cursor
                            },
                            None => {
                                error!("No symbol in list for {:?}", chain_list);
                                return Ok(vec![]);
                            } // Nothing under cursor
                        };
                        while let Some(next_item) = chain_list.next() {
                            let members_and_methods =
                                self.list_members_and_methods(&current_symbol);
                            let symbol =
                                match members_and_methods.iter().find(|e| e.label == next_item.0) {
                                    Some(next_symbol) => next_symbol.clone(),
                                    None => {
                                        return Err(SymbolError::InternalErr(format!(
                                            "Failed to find symbol {} for struct {}",
                                            next_item.0, current_symbol.label
                                        )))
                                    }
                                };
                            // find next element
                            if let ShaderSymbolData::Variables { ty } = &symbol.data {
                                match symbol_list.find_type_symbol(ty) {
                                    Some(ty_symbol) => current_symbol = ty_symbol,
                                    None => {
                                        return Ok(vec![]);
                                    }
                                }
                            } else {
                                error!("Not variable 2 {:?}", symbol);
                                return Ok(vec![]); // Nothing valid under cursor
                            }
                        }
                        let members_and_methods = self.list_members_and_methods(&current_symbol);
                        return Ok(members_and_methods
                            .into_iter()
                            .map(|s| {
                                let completion_kind = if let ShaderSymbolData::Functions {
                                    signatures: _,
                                } = &s.data
                                {
                                    CompletionItemKind::FUNCTION
                                } else {
                                    CompletionItemKind::VARIABLE
                                };
                                convert_completion_item(
                                    cached_file.shading_language,
                                    s,
                                    completion_kind,
                                )
                            })
                            .collect());
                    }
                    Err(err) => {
                        if let SymbolError::NoSymbol = err {
                            Ok(vec![])
                        } else {
                            Err(err)
                        }
                    }
                }
            }
            None => Ok(symbol_list
                .into_iter()
                .map(|(symbol_list, ty)| {
                    symbol_list
                        .into_iter()
                        .map(|s| {
                            convert_completion_item(
                                cached_file.shading_language,
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
