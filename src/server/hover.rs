use std::io::{BufRead, BufReader};

use lsp_types::{Hover, HoverContents, MarkupContent, Position, Url};
use regex::Regex;

use crate::{
    server::ServerLanguage,
    shaders::{
        shader::ShadingLanguage, shader_error::ValidatorError, symbols::symbols::ShaderPosition, validator::validator::ValidationParams
    },
};

impl ServerLanguage {
    pub fn recolt_hover(
        &mut self,
        uri: &Url,
        shading_language: ShadingLanguage,
        content: String,
        position: Position,
    ) -> Result<Option<Hover>, ValidatorError> {
        let word_and_range = get_word_range_at_position(content.clone(), position);
        match word_and_range {
            Some(word_and_range) => {
                let file_path = uri
                    .to_file_path()
                    .expect(format!("Failed to convert {} to a valid path.", uri).as_str());
                let validation_params = ValidationParams::new(
                    self.config.includes.clone(),
                    self.config.defines.clone(),
                );

                let symbol_provider = self.get_symbol_provider(shading_language);
                let completion = symbol_provider.capture(&content, &file_path, &validation_params, Some(ShaderPosition {
                    file_path: file_path.clone(),
                    line: position.line as u32,
                    pos: position.character as u32,
                }));

                let symbols = completion.find_symbols(word_and_range.0);
                if symbols.is_empty() {
                    Ok(None)
                } else {
                    let symbol = symbols[0];
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
                                if symbols.len() > 1 {
                                    format!("(+{} symbol)\n\n", symbols.len() - 1)
                                } else {
                                    "".into()
                                },
                                description,
                                link
                            ),
                        }),
                        range: Some(word_and_range.1),
                    }))
                }
            }
            None => Ok(None),
        }
    }
}
pub fn get_word_range_at_position(
    shader: String,
    position: Position,
) -> Option<(String, lsp_types::Range)> {
    // vscode getWordRangeAtPosition does something similar
    let reader = BufReader::new(shader.as_bytes());
    let line = reader
        .lines()
        .nth(position.line as usize)
        .expect("Text position is out of bounds")
        .expect("Could not read line");
    let regex =
        Regex::new("(-?\\d*\\.\\d\\w*)|([^\\`\\~\\!\\@\\#\\%\\^\\&\\*\\(\\)\\-\\=\\+\\[\\{\\]\\}\\\\|\\;\\:\\'\\\"\\,\\.<>\\/\\?\\s]+)").expect("Failed to init regex");
    for capture in regex.captures_iter(line.as_str()) {
        let word = capture.get(0).expect("Failed to get word");
        if position.character >= word.start() as u32 && position.character <= word.end() as u32 {
            return Some((
                line[word.start()..word.end()].into(),
                lsp_types::Range::new(
                    lsp_types::Position::new(position.line, word.start() as u32),
                    lsp_types::Position::new(position.line, word.end() as u32),
                ),
            ));
        }
    }
    None
}
