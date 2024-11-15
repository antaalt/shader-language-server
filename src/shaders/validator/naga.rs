use naga::{
    front::wgsl::{self, ParseError},
    valid::{Capabilities, ValidationFlags},
};
use std::path::Path;

use crate::shaders::{
    include::Dependencies,
    shader_error::{ShaderDiagnostic, ShaderDiagnosticList, ShaderErrorSeverity, ValidatorError},
};

use super::validator::{ValidationParams, Validator};

pub struct Naga {
    validator: naga::valid::Validator,
}

impl Naga {
    pub fn new() -> Self {
        Self {
            validator: naga::valid::Validator::new(ValidationFlags::all(), Capabilities::all()),
        }
    }
    fn from_parse_err(err: ParseError, src: &str) -> ShaderDiagnostic {
        let error = err.emit_to_string(src);
        let loc = err.location(src);
        if let Some(loc) = loc {
            ShaderDiagnostic {
                file_path: None,
                severity: ShaderErrorSeverity::Error,
                error,
                line: loc.line_number,
                pos: loc.line_position,
            }
        } else {
            ShaderDiagnostic {
                file_path: None,
                severity: ShaderErrorSeverity::Error,
                error,
                line: 0,
                pos: 0,
            }
        }
    }
}
impl Validator for Naga {
    fn validate_shader(
        &mut self,
        shader_content: String,
        _file_path: &Path,
        _params: ValidationParams,
        _include_callback: &mut dyn FnMut(&Path) -> Option<String>,
    ) -> Result<(ShaderDiagnosticList, Dependencies), ValidatorError> {
        let module = match wgsl::parse_str(&shader_content)
            .map_err(|err| Self::from_parse_err(err, &shader_content))
        {
            Ok(module) => module,
            Err(diag) => {
                return Ok((ShaderDiagnosticList::from(diag), Dependencies::new()));
            }
        };

        if let Err(error) = self.validator.validate(&module) {
            let mut list = ShaderDiagnosticList::empty();
            for (span, _) in error.spans() {
                let loc = span.location(&shader_content);
                list.push(ShaderDiagnostic {
                    file_path: None,
                    severity: ShaderErrorSeverity::Error,
                    error: error.emit_to_string(""),
                    line: loc.line_number,
                    pos: loc.line_position,
                });
            }
            if list.is_empty() {
                Err(ValidatorError::internal(
                    error.emit_to_string(&shader_content),
                ))
            } else {
                Ok((list, Dependencies::new()))
            }
        } else {
            Ok((ShaderDiagnosticList::empty(), Dependencies::new()))
        }
    }
}
