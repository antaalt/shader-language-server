use std::{collections::HashMap, ffi::OsStr};

use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, MarkupContent, Position, Url,
};

use crate::{
    server::ServerLanguage,
    shaders::{
        shader::ShadingLanguage, shader_error::ValidatorError, symbols::symbols::{ShaderPosition, ShaderSymbol},
        validator::validator::ValidationParams,
    },
};

impl ServerLanguage {
    pub fn recolt_completion(
        &self,
        uri: &Url,
        shading_language: ShadingLanguage,
        shader_source: String,
        position: Position,
    ) -> Result<Vec<CompletionItem>, ValidatorError> {
        let file_path = uri
            .to_file_path()
            .expect(format!("Failed to convert {} to a valid path.", uri).as_str());
        let validation_params =
            ValidationParams::new(self.config.includes.clone(), self.config.defines.clone());
        let symbol_provider = self.get_symbol_provider(shading_language);
        let completion = symbol_provider.capture(&shader_source, &file_path, &validation_params, Some(ShaderPosition {
            file_path: file_path.clone(),
            line: position.line as u32,
            pos: position.character as u32,
        }));
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
            set.iter().map(|e| e.1.clone()).collect()
        };
        let mut items = Vec::<CompletionItem>::new();
        items.append(
            &mut filter_symbols(completion.functions)
                .iter()
                .map(|s| {
                    convert_completion_item(
                        shading_language,
                        s.0.clone(),
                        CompletionItemKind::FUNCTION,
                        Some(s.1.clone()),
                    )
                })
                .collect(),
        );
        items.append(
            &mut filter_symbols(completion.constants)
                .iter()
                .map(|s| {
                    convert_completion_item(
                        shading_language,
                        s.0.clone(),
                        CompletionItemKind::CONSTANT,
                        Some(s.1.clone()),
                    )
                })
                .collect(),
        );
        items.append(
            &mut filter_symbols(completion.variables)
                .iter()
                .map(|s| {
                    convert_completion_item(
                        shading_language,
                        s.0.clone(),
                        CompletionItemKind::VARIABLE,
                        Some(s.1.clone()),
                    )
                })
                .collect(),
        );
        items.append(
            &mut filter_symbols(completion.types)
                .iter()
                .map(|s| {
                    convert_completion_item(
                        shading_language,
                        s.0.clone(),
                        CompletionItemKind::TYPE_PARAMETER,
                        Some(s.1.clone()),
                    )
                })
                .collect(),
        );
        Ok(items)
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
