use naga::{
    front::wgsl::{self, ParseError},
    valid::{Capabilities, ValidationFlags},
};
use std::path::Path;

use crate::{
    common::{
        get_default_shader_completion, ShaderSymbol, ShaderSymbolList, ShadingLanguage,
        ValidationParams, Validator,
    },
    include::Dependencies,
    shader_error::{ShaderDiagnostic, ShaderDiagnosticList, ShaderErrorSeverity, ValidatorError},
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
                relative_path: None,
                severity: ShaderErrorSeverity::Error,
                error,
                line: loc.line_number,
                pos: loc.line_position,
            }
        } else {
            ShaderDiagnostic {
                relative_path: None,
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
                    relative_path: None,
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

    fn get_shader_completion(
        &mut self,
        shader_content: String,
        _file_path: &Path,
        _params: ValidationParams,
    ) -> Result<ShaderSymbolList, ValidatorError> {
        let module = match wgsl::parse_str(&shader_content)
            .map_err(|err| Self::from_parse_err(err, &shader_content))
        {
            Ok(module) => module,
            Err(_) => {
                // Do not fail, just return default completion items.
                // TODO: should cache latest completion for this file instead & return error to be handled by server.
                return Ok(get_default_shader_completion(ShadingLanguage::Wgsl));
            }
        };
        // TODO: parse https://webgpu.rocks/wgsl/functions/logic-array/
        let mut completion = get_default_shader_completion(ShadingLanguage::Wgsl);

        for (_, ty) in module.types.iter() {
            if let Some(name) = &ty.name {
                completion.functions.push(ShaderSymbol::new(
                    name.clone(),
                    "".to_string(),
                    "".to_string(),
                    Vec::new(),
                ));
            }
        }

        for (_, var) in module.constants.iter() {
            if let Some(name) = &var.name {
                completion.functions.push(ShaderSymbol::new(
                    name.clone(),
                    "".to_string(),
                    "".to_string(),
                    Vec::new(),
                ));
            }
        }

        for (_, var) in module.global_variables.iter() {
            if let Some(name) = &var.name {
                completion.functions.push(ShaderSymbol::new(
                    name.clone(),
                    "".to_string(),
                    "".to_string(),
                    Vec::new(),
                ));
            }
        }

        for (_, f) in module.functions.iter() {
            if let Some(name) = &f.name {
                completion.functions.push(ShaderSymbol::new(
                    name.clone(),
                    "".to_string(),
                    "".to_string(),
                    Vec::new(),
                ));
            }
        }

        Ok(completion)
    }
}
