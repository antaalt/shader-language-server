use std::rc::Rc;

use crate::shaders::symbols::symbols::{
    ShaderPosition, ShaderRange, ShaderSymbolData, SymbolError,
};

use lsp_types::{GotoDefinitionResponse, Position, Url};

use super::{common::shader_range_to_lsp_range, ServerFileCacheHandle, ServerLanguageData};

impl ServerLanguageData {
    pub fn recolt_goto(
        &mut self,
        uri: &Url,
        cached_file: ServerFileCacheHandle,
        position: Position,
    ) -> Result<Option<GotoDefinitionResponse>, SymbolError> {
        let file_path = uri.to_file_path().unwrap();
        let shader_position = ShaderPosition {
            file_path: file_path.clone(),
            line: position.line as u32,
            pos: position.character as u32,
        };
        let all_symbol_list = self.get_all_symbols(Rc::clone(&cached_file));
        let cached_file = cached_file.borrow();
        match self
            .symbol_provider
            .get_word_range_at_position(&cached_file.symbol_tree, shader_position.clone())
        {
            Ok((word, word_range)) => {
                let symbol_list = all_symbol_list.filter_scoped_symbol(shader_position);
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
                                        target_uri: Url::from_file_path(&target.file_path).unwrap(),
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
                                            .unwrap(),
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
            Err(err) => {
                if let SymbolError::NoSymbol = err {
                    Ok(None)
                } else {
                    Err(err)
                }
            }
        }
    }
}
