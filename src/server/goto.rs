use crate::{
    server::ServerLanguage,
    shaders::symbols::symbols::{ShaderPosition, ShaderRange, ShaderSymbolData},
};

use lsp_types::{GotoDefinitionResponse, Position, Url};

use crate::shaders::shader_error::ValidatorError;

use super::{hover::shader_range_to_lsp_range, ServerFileCacheHandle};

impl ServerLanguage {
    pub fn recolt_goto(
        &mut self,
        uri: &Url,
        cached_file: ServerFileCacheHandle,
        position: Position,
    ) -> Result<Option<GotoDefinitionResponse>, ValidatorError> {
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
            Some((word, word_range)) => {
                let symbol_list = cached_file
                    .symbol_cache
                    .filter_scoped_symbol(shader_position);
                let matching_symbols = symbol_list.find_symbols(word);
                Ok(Some(GotoDefinitionResponse::Link(
                    matching_symbols
                        .iter()
                        .filter_map(|symbol| {
                            if let ShaderSymbolData::Link { target } = &symbol.data {
                                match &symbol.range {
                                    // _range here should be equal to selected_range.
                                    Some(_range) => Some(lsp_types::LocationLink {
                                        origin_selection_range: Some(shader_range_to_lsp_range(
                                            &word_range,
                                        )),
                                        target_uri: Url::from_file_path(&target.file_path)
                                            .expect("Failed to convert file path"),
                                        target_range: shader_range_to_lsp_range(&ShaderRange::new(
                                            target.clone(),
                                            target.clone(),
                                        )),
                                        target_selection_range: shader_range_to_lsp_range(
                                            &ShaderRange::new(target.clone(), target.clone()),
                                        ),
                                    }),
                                    None => None,
                                }
                            } else {
                                match &symbol.range {
                                    Some(range) => Some(lsp_types::LocationLink {
                                        origin_selection_range: Some(shader_range_to_lsp_range(
                                            &word_range,
                                        )),
                                        target_uri: Url::from_file_path(&range.start.file_path)
                                            .expect("Failed to convert file path"),
                                        target_range: shader_range_to_lsp_range(range),
                                        target_selection_range: shader_range_to_lsp_range(range),
                                    }),
                                    None => None,
                                }
                            }
                        })
                        .collect(),
                )))
            }
            None => Ok(None),
        }
    }
}
