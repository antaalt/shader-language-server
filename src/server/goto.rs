use crate::{server::ServerLanguage, shaders::symbols::symbols::ShaderPosition};

use lsp_types::{GotoDefinitionResponse, Position, Url};

use crate::shaders::{shader::ShadingLanguage, shader_error::ValidatorError};

use super::hover::get_word_range_at_position;

impl ServerLanguage {
    pub fn recolt_goto(
        &mut self,
        uri: &Url,
        shading_language: ShadingLanguage,
        content: String,
        position: Position,
    ) -> Result<Option<GotoDefinitionResponse>, ValidatorError> {
        let word_and_range = get_word_range_at_position(&content, position);
        match word_and_range {
            Some(word_and_range) => {
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

                let symbols = completion.find_symbols(word_and_range.0);
                if symbols.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(GotoDefinitionResponse::Array(
                        symbols
                            .iter()
                            .filter_map(|symbol| match &symbol.range {
                                Some(range) => Some(lsp_types::Location {
                                    uri: Url::from_file_path(&range.start.file_path)
                                        .expect("Failed to convert file path"),
                                    range: lsp_types::Range::new(
                                        lsp_types::Position::new(range.start.line, range.start.pos),
                                        lsp_types::Position::new(range.start.line, range.start.pos),
                                    ),
                                }),
                                None => None,
                            })
                            .collect(),
                    )))
                }
            }
            None => Ok(None),
        }
    }
}
