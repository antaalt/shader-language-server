use std::{io::{BufRead, BufReader}, path::PathBuf};

use lsp_types::{Hover, HoverContents, MarkupContent, Position, Url};
use regex::Regex;

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
            Some((_selected_range, symbol_list)) => if symbol_list.len() == 0 {
                Ok(None)
            } else {
                let symbol = &symbol_list[0];
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
                            if symbol_list.len() > 1 {
                                format!("(+{} symbol)\n\n", symbol_list.len() - 1)
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
            None => Ok(None)
        }
    }
}
pub fn get_word_range_at_position(
    shader: &String,
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
