use crate::{server::ServerLanguage, shaders::symbols::symbols::ShaderPosition};

use lsp_types::{GotoDefinitionResponse, Position, Url};

use crate::shaders::{shader::ShadingLanguage, shader_error::ValidatorError};

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
        let symbols = self
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
            );
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
}
