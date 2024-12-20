use std::path::Path;

use shader_sense::symbols::symbols::{ShaderPosition, ShaderRange};

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

pub fn lsp_range_to_shader_range(range: &lsp_types::Range, file_path: &Path) -> ShaderRange {
    ShaderRange {
        start: ShaderPosition {
            file_path: file_path.into(),
            line: range.start.line,
            pos: range.start.character,
        },
        end: ShaderPosition {
            file_path: file_path.into(),
            line: range.end.line,
            pos: range.end.character,
        },
    }
}

// Handle non-utf8 characters
pub fn read_string_lossy(file_path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    match std::fs::read_to_string(file_path) {
        Ok(content) => Ok(content),
        Err(err) => match err.kind() {
            std::io::ErrorKind::InvalidData => {
                // Load non utf8 file as lossy string.
                log::warn!(
                    "Non UTF8 characters detected in file {}. Loaded as lossy string.",
                    file_path.display()
                );
                let mut file = std::fs::File::open(file_path).unwrap();
                let mut buf = vec![];
                file.read_to_end(&mut buf).unwrap();
                Ok(String::from_utf8_lossy(&buf).into())
            }
            _ => Err(err),
        },
    }
}
