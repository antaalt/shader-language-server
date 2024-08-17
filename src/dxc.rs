use hassle_rs::*;
use std::path::Path;

use crate::{
    common::{
        get_default_shader_completion, ShaderSymbolList, ShadingLanguage, ValidationParams,
        Validator,
    },
    include::IncludeHandler,
    shader_error::{
        ShaderDiagnostic, ShaderDiagnosticList, ShaderError, ShaderErrorSeverity, ValidatorError,
    },
};

pub struct Dxc {
    compiler: hassle_rs::DxcCompiler,
    library: hassle_rs::DxcLibrary,

    validator: Option<hassle_rs::DxcValidator>,
    dxil: Option<hassle_rs::wrapper::Dxil>,

    #[allow(dead_code)] // Need to keep dxc alive while dependencies created
    dxc: hassle_rs::wrapper::Dxc,
}

impl hassle_rs::wrapper::DxcIncludeHandler for IncludeHandler {
    fn load_source(&mut self, filename: String) -> Option<String> {
        let path = Path::new(filename.as_str());
        self.search_in_includes(&path)
    }
}

impl Dxc {
    pub fn new() -> Result<Self, hassle_rs::HassleError> {
        let dxc = hassle_rs::Dxc::new(None)?;
        let library = dxc.create_library()?;
        let compiler = dxc.create_compiler()?;
        let (dxil, validator) = match Dxil::new(None) {
            Ok(dxil) => {
                let validator_option = match dxil.create_validator() {
                    Ok(validator) => Some(validator),
                    Err(_) => None,
                };
                (Some(dxil), validator_option)
            }
            Err(_) => (None, None),
        };
        Ok(Self {
            dxc,
            compiler,
            library,
            dxil,
            validator,
        })
    }
    fn parse_dxc_errors(errors: &String) -> Result<ShaderDiagnosticList, ValidatorError> {
        let mut shader_error_list = ShaderDiagnosticList::empty();

        let reg = regex::Regex::new(r"(?m)^(.*?:\d+:\d+: .*:.*?)$")?;
        let mut starts = Vec::new();
        for capture in reg.captures_iter(errors.as_str()) {
            if let Some(pos) = capture.get(0) {
                starts.push(pos.start());
            }
        }
        starts.push(errors.len());
        let internal_reg = regex::Regex::new(r"(?s)^(.*?):(\d+):(\d+): (.*?):(.*)")?;
        for start in 0..starts.len() - 1 {
            let first = starts[start];
            let length = starts[start + 1] - starts[start];
            let block: String = errors.chars().skip(first).take(length).collect();
            if let Some(capture) = internal_reg.captures(block.as_str()) {
                let filename = capture.get(1).map_or("", |m| m.as_str());
                let line = capture.get(2).map_or("", |m| m.as_str());
                let pos = capture.get(3).map_or("", |m| m.as_str());
                let level = capture.get(4).map_or("", |m| m.as_str());
                let msg = capture.get(5).map_or("", |m| m.as_str());
                shader_error_list.push(ShaderDiagnostic {
                    filename: Some(String::from(filename)),
                    severity: match level {
                        "error" => ShaderErrorSeverity::Error,
                        "warning" => ShaderErrorSeverity::Warning,
                        "note" => ShaderErrorSeverity::Information,
                        "hint" => ShaderErrorSeverity::Hint,
                        _ => ShaderErrorSeverity::Error,
                    },
                    error: String::from(msg),
                    line: line.parse::<u32>().unwrap_or(0),
                    pos: pos.parse::<u32>().unwrap_or(0),
                });
            }
        }

        if shader_error_list.is_empty() {
            Err(ValidatorError::internal(format!(
                "Failed to parse errors: {}",
                errors
            )))
        } else {
            Ok(shader_error_list)
        }
    }
    fn from_hassle_error(&self, error: HassleError) -> ShaderError {
        match error {
            HassleError::CompileError(err) => match Dxc::parse_dxc_errors(&err) {
                Ok(diagnostic) => ShaderError::DiagnosticList(diagnostic),
                Err(error) => ShaderError::Validator(error),
            },
            HassleError::ValidationError(err) => {
                ShaderError::DiagnosticList(ShaderDiagnosticList::from(ShaderDiagnostic {
                    filename: None, // TODO: pass filename as arg with some ShaderErrorContext
                    severity: ShaderErrorSeverity::Error,
                    error: err.to_string(),
                    line: 0,
                    pos: 0,
                }))
            }
            HassleError::LibLoadingError(err) => {
                ShaderError::Validator(ValidatorError::internal(err.to_string()))
            }
            HassleError::LoadLibraryError { filename, inner } => {
                ShaderError::Validator(ValidatorError::internal(format!(
                    "Failed to load library {}: {}",
                    filename.display(),
                    inner.to_string()
                )))
            }
            HassleError::Win32Error(err) => ShaderError::Validator(ValidatorError::internal(
                format!("Win32 error: HRESULT={}", err),
            )),
            HassleError::WindowsOnly(err) => ShaderError::Validator(ValidatorError::internal(
                format!("Windows only error: {}", err),
            )),
        }
    }
}
impl Validator for Dxc {
    fn validate_shader(
        &mut self,
        shader_source: String,
        file_path: &Path,
        params: ValidationParams,
    ) -> Result<ShaderDiagnosticList, ValidatorError> {
        let file_name = self.get_file_name(file_path);
        let cwd = self.get_cwd(file_path);

        let blob = self
            .library
            .create_blob_with_encoding_from_str(&shader_source)
            .map_err(|e| self.from_hassle_error(e))?;

        let defines_copy = params.defines.clone();
        let defines: Vec<(&str, Option<&str>)> = defines_copy
            .iter()
            .map(|v| (&v.0 as &str, Some(&v.1 as &str)))
            .collect();

        let result = self.compiler.compile(
            &blob,
            file_name.as_str(),
            "", // TODO: Could have a command to validate specific entry point (specify stage & entry point)
            "lib_6_5",
            &[], // TODO: should control this from settings (-enable-16bit-types)
            Some(&mut IncludeHandler::new(cwd, params.includes)),
            &defines,
        );

        match result {
            Ok(dxc_result) => {
                let result_blob = dxc_result
                    .get_result()
                    .map_err(|e| self.from_hassle_error(e))?;
                // Skip validation if dxil.dll does not exist.
                if let (Some(_dxil), Some(validator)) = (&self.dxil, &self.validator) {
                    let data = result_blob.to_vec();
                    let blob_encoding = self
                        .library
                        .create_blob_with_encoding(data.as_ref())
                        .map_err(|e| self.from_hassle_error(e))?;

                    match validator.validate(blob_encoding.into()) {
                        Ok(_) => Ok(ShaderDiagnosticList::empty()),
                        Err(dxc_err) => {
                            //let error_blob = dxc_err.0.get_error_buffer().map_err(|e| self.from_hassle_error(e))?;
                            //let error_emitted = self.library.get_blob_as_string(&error_blob.into()).map_err(|e| self.from_hassle_error(e))?;
                            match self.from_hassle_error(dxc_err.1) {
                                ShaderError::Validator(err) => Err(err),
                                ShaderError::DiagnosticList(diag) => Ok(diag),
                            }
                        }
                    }
                } else {
                    Ok(ShaderDiagnosticList::empty())
                }
            }
            Err((dxc_result, _hresult)) => {
                let error_blob = dxc_result
                    .get_error_buffer()
                    .map_err(|e| self.from_hassle_error(e))?;
                let error_emitted = self
                    .library
                    .get_blob_as_string(&error_blob.into())
                    .map_err(|e| self.from_hassle_error(e))?;
                match self.from_hassle_error(HassleError::CompileError(error_emitted)) {
                    ShaderError::Validator(error) => Err(error),
                    ShaderError::DiagnosticList(diag) => Ok(diag),
                }
            }
        }
    }

    fn get_shader_completion(
        &mut self,
        shader_content: String,
        file_path: &Path,
        params: ValidationParams,
    ) -> Result<ShaderSymbolList, ValidatorError> {
        let file_name = self.get_file_name(file_path);
        let cwd = self.get_cwd(file_path);

        // TODO: could parse
        // https://learn.microsoft.com/en-ca/windows/win32/direct3dhlsl/dx-graphics-hlsl-intrinsic-functions
        // https://learn.microsoft.com/en-ca/windows/win32/direct3dhlsl/dx-graphics-hlsl-semantics
        let completion = get_default_shader_completion(ShadingLanguage::Hlsl);

        let blob = self
            .library
            .create_blob_with_encoding_from_str(&shader_content)
            .map_err(|e| self.from_hassle_error(e))?;

        let result = self.compiler.compile(
            &blob,
            file_name.as_str(),
            "",
            "lib_6_5",
            &[],
            Some(&mut IncludeHandler::new(cwd, params.includes)),
            &[],
        );

        match result {
            Ok(dxc_result) => {
                let result_blob = dxc_result
                    .get_result()
                    .map_err(|e| self.from_hassle_error(e))?;
                let data = result_blob.to_vec();
                let blob_encoding = self
                    .library
                    .create_blob_with_encoding(data.as_ref())
                    .map_err(|e| self.from_hassle_error(e))?;
                let reflector = self
                    .dxc
                    .create_reflector()
                    .map_err(|e| self.from_hassle_error(e))?;
                let reflection = reflector
                    .reflect(blob_encoding.into())
                    .map_err(|e| self.from_hassle_error(e))?;
                // Hassle capabilities on this seems limited for now...
                // Would need to create a PR to add interface for other API.
                reflection.thread_group_size();

                Ok(completion)
            }
            Err((_dxc_result, _hresult)) => Err(ValidatorError::internal(String::from(
                "Failed to get reflection data from shader",
            ))),
        }
    }
}
