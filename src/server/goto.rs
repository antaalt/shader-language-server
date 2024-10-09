use crate::{server::ServerLanguage, shaders::symbols::symbols::{ShaderPosition, ShaderRange, ShaderSymbolData}};

use lsp_types::{GotoDefinitionResponse, Position, Url};

use crate::shaders::{shader::ShadingLanguage, shader_error::ValidatorError};

use super::hover::shader_range_to_lsp_range;

impl ServerLanguage {
    pub fn recolt_goto(
        &mut self,
        uri: &Url,
        shading_language: ShadingLanguage,
        content: String,
        position: Position,
    ) -> Result<Option<GotoDefinitionResponse>, ValidatorError> {
        let file_path = uri
            .to_file_path()
            .expect(format!("Failed to convert {} to a valid path.", uri).as_str());
        let validation_params = self.config.into_validation_params();
        match self
            .get_symbol_provider(shading_language)
            .get_symbols_at_position(
                &content,
                &file_path,
                &validation_params,
                ShaderPosition {
                    file_path: file_path.clone(),
                    line: position.line as u32,
                    pos: position.character as u32,
                },
            ) {
            Some((selected_range, symbol_list)) => Ok(Some(GotoDefinitionResponse::Link(
                symbol_list
                    .iter()
                    .filter_map(|symbol| if let ShaderSymbolData::Link { target } = &symbol.data {
                        match &symbol.range {
                            // _range here should be equal to selected_range.
                            Some(_range) => Some(lsp_types::LocationLink {
                                origin_selection_range: Some(shader_range_to_lsp_range(&selected_range)),
                                target_uri: Url::from_file_path(&target.file_path)
                                    .expect("Failed to convert file path"),
                                target_range: shader_range_to_lsp_range(&ShaderRange::new(target.clone(), target.clone())),
                                target_selection_range: shader_range_to_lsp_range(&ShaderRange::new(target.clone(), target.clone())),
                            }),
                            None => None,
                        }
                    } else {
                        match &symbol.range {
                            Some(range) => Some(lsp_types::LocationLink {
                                origin_selection_range: Some(shader_range_to_lsp_range(&selected_range)),
                                target_uri: Url::from_file_path(&range.start.file_path)
                                    .expect("Failed to convert file path"),
                                target_range: shader_range_to_lsp_range(range),
                                target_selection_range: shader_range_to_lsp_range(range),
                            }),
                            None => None,
                        }
                    })
                    .collect(),
            ))),
            None => Ok(None),
        }
    }
}
