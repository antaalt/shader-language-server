use std::path::PathBuf;

use lsp_types::{Hover, HoverContents, MarkupContent, Position, Url};

use crate::{
    server::ServerLanguage,
    shaders::{
        shader_error::ValidatorError,
        symbols::symbols::{ShaderPosition, ShaderRange},
    },
};

use super::ServerFileCacheHandle;
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
pub fn lsp_range_to_shader_range(range: &lsp_types::Range, file_path: &PathBuf) -> ShaderRange {
    ShaderRange {
        start: ShaderPosition {
            file_path: file_path.clone(),
            line: range.start.line,
            pos: range.start.character,
        },
        end: ShaderPosition {
            file_path: file_path.clone(),
            line: range.end.line,
            pos: range.end.character,
        },
    }
}

impl ServerLanguage {
    pub fn recolt_hover(
        &mut self,
        uri: &Url,
        cached_file: ServerFileCacheHandle,
        position: Position,
    ) -> Result<Option<Hover>, ValidatorError> {
        let file_path = Self::to_file_path(uri);
        let shader_position = ShaderPosition {
            file_path: file_path.clone(),
            line: position.line as u32,
            pos: position.character as u32,
        };
        let cached_file = cached_file.borrow();
        match self
            .get_symbol_provider(cached_file.shading_language)
            .get_word_range_at_position(&cached_file.content, &file_path, shader_position.clone())
        {
            // word_range should be the same as symbol range
            Some((word, _word_range)) => match self.get_watched_file(uri) {
                Some(target_cached_file) => {
                    let target_cached_file = target_cached_file.borrow();
                    let symbol_list = target_cached_file
                        .symbol_cache
                        .filter_scoped_symbol(shader_position);
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
                                    target_cached_file.shading_language.to_string(),
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
                                Some(range) => {
                                    if range.start.file_path == *file_path {
                                        Some(shader_range_to_lsp_range(range))
                                    } else {
                                        None
                                    }
                                }
                            },
                        }))
                    }
                }
                None => Ok(None),
            },
            None => Ok(None),
        }
    }
}
