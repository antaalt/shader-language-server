use crate::server::ServerLanguage;

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
        let word_and_range = get_word_range_at_position(content.clone(), position);
        match word_and_range {
            Some(word_and_range) => {
                let file_path = uri
                    .to_file_path()
                    .expect(format!("Failed to convert {} to a valid path.", uri).as_str());
                let includes = self.config.includes.clone();

                let symbol_provider = self.get_symbol_provider(shading_language);
                let completion = symbol_provider.capture(&content, &file_path, includes);

                let symbols = completion.find_symbols(word_and_range.0);
                if symbols.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(GotoDefinitionResponse::Array(
                        symbols
                            .iter()
                            .filter_map(|symbol| match &symbol.position {
                                Some(pos) => Some(lsp_types::Location {
                                    uri: Url::from_file_path(&pos.file_path)
                                        .expect("Failed to convert file path"),
                                    range: lsp_types::Range::new(
                                        lsp_types::Position::new(pos.line, pos.pos),
                                        lsp_types::Position::new(pos.line, pos.pos),
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
