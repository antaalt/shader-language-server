use naga::{
    front::wgsl::{self, ParseError},
    valid::{Capabilities, ValidationFlags},
};
use std::path::Path;

use crate::{
    common::{ShaderTree, ValidationParams, Validator},
    shader_error::{ShaderDiagnostic, ShaderDiagnosticList, ShaderError, ShaderErrorSeverity, ValidatorError},
};

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
                filename: None,
                severity: ShaderErrorSeverity::Error,
                error,
                line: loc.line_number,
                pos: loc.line_position,
            }
        } else {
            ShaderDiagnostic {
                filename: None,
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
        file_path: &Path,
        _params: ValidationParams,
    ) -> Result<ShaderDiagnosticList, ValidatorError> {
        let file_name = String::from(file_path.file_name().unwrap_or_default().to_string_lossy());

        let module = match wgsl::parse_str(&shader_content).map_err(|err| Self::from_parse_err(err, &shader_content)) {
            Ok(module) => module,
            Err(diag) => {
                return Ok(ShaderDiagnosticList::from(diag));
            }
        };
        
        if let Err(error) = self.validator.validate(&module) {
            let mut list = ShaderDiagnosticList::empty();
            for (span, _) in error.spans() {
                let loc = span.location(&shader_content);
                list.push(ShaderDiagnostic {
                    filename: Some(file_name.clone()),
                    severity: ShaderErrorSeverity::Error,
                    error: error.emit_to_string(""),
                    line: loc.line_number,
                    pos: loc.line_position,
                });
            }
            if list.is_empty() {
                Err(ValidatorError::internal(error.emit_to_string(&shader_content)))
            } else {
                Ok(list)
            }
        } else {
            Ok(ShaderDiagnosticList::empty())
        }
    }

    fn get_shader_completion(
        &mut self,
        shader_content: String,
        _file_path: &Path,
        _params: ValidationParams,
    ) -> Result<ShaderTree, ValidatorError> {
        let module = match wgsl::parse_str(&shader_content).map_err(|err| Self::from_parse_err(err, &shader_content)) {
            Ok(module) => module,
            Err(_) => {
                // Do not fail, just return empty completion items.
                // TODO: should pick latest completion for this file instead.
                return Ok(ShaderTree::default());
            }
        };

        let mut types = Vec::new();
        let mut global_variables = Vec::new();
        let mut functions = Vec::new();

        for (_, ty) in module.types.iter() {
            if let Some(name) = &ty.name {
                types.push(name.clone())
            }
        }

        for (_, var) in module.constants.iter() {
            if let Some(name) = &var.name {
                global_variables.push(name.clone())
            }
        }

        for (_, f) in module.functions.iter() {
            if let Some(name) = &f.name {
                functions.push(name.clone())
            }
        }

        Ok(ShaderTree {
            types,
            global_variables,
            functions,
        })
    }
}
