use lsp_types::{Hover, HoverContents, MarkupContent, Position, Url};

use crate::{
    server::ServerLanguage,
    shaders::{
        shader::ShadingLanguage,
        shader_error::ValidatorError,
        symbols::symbols::{ShaderPosition, ShaderRange},
    },
};
pub fn shader_range_to_lsp_range(range: &ShaderRange) -> lsp_types::Range {
    lsp_types::Range {
        start: lsp_types::Position {
            line: range.start.line,
            character: range.start.pos,
        },
        end: lsp_types::Position {
            line: range.end.line,
            character: range.end.pos,
        },
    }
}

impl ServerLanguage {
    pub fn recolt_hover(
        &mut self,
        uri: &Url,
        shading_language: ShadingLanguage,
        content: String,
        position: Position,
    ) -> Result<Option<Hover>, ValidatorError> {
        let file_path = uri
            .to_file_path()
            .expect(format!("Failed to convert {} to a valid path.", uri).as_str());
        let shader_position = ShaderPosition {
            file_path: file_path.clone(),
            line: position.line as u32,
            pos: position.character as u32,
        };
        match self.get_symbol_provider(shading_language).get_word_range_at_position(&content, &file_path, shader_position.clone()) {
            // word_range should be the same as symbol range
            Some((word, _word_range)) => {
                match self.get_watched_file(uri) {
                    Some(cached_file) => {
                        let symbol_list = cached_file.symbol_cache.filter_scoped_symbol(shader_position);
                        let matching_symbols = symbol_list.find_symbols(word);
                        if matching_symbols.len() == 0 {
                            Ok(None)
                        } else {
                            let symbol = &matching_symbols[0];
                            let label = symbol.format();
                            let description = symbol.description.clone();
                            let link = match &symbol.link {
                                Some(link) => format!("[Online documentation]({})", link),
                                None => "".into(),
                            };
                            Ok(Some(Hover {
                                contents: HoverContents::Markup(MarkupContent {
                                    kind: lsp_types::MarkupKind::Markdown,
                                    value: format!(
                                        "```{}\n{}\n```\n{}{}\n\n{}",
                                        shading_language.to_string(),
                                        label,
                                        if matching_symbols.len() > 1 {
                                            format!("(+{} symbol)\n\n", matching_symbols.len() - 1)
                                        } else {
                                            "".into()
                                        },
                                        description,
                                        link
                                    ),
                                }),
                                range: match &symbol.range {
                                    None => None,
                                    Some(range) => if range.start.file_path == *file_path {
                                        Some(shader_range_to_lsp_range(range))
                                    } else {
                                        None
                                    }
                                },
                            }))
                        }
                    },
                    None => Ok(None),
                }
            },
            None => Ok(None)
        }
    }
}
