use std::{collections::HashMap, path::Path};

use crate::shaders::{include::Dependencies, shader_error::{ShaderDiagnosticList, ValidatorError}, symbols::symbols::ShaderSymbolList};



pub struct ValidationParams {
    pub includes: Vec<String>,
    pub defines: HashMap<String, String>,
}

impl ValidationParams {
    pub fn new(includes: Vec<String>, defines: HashMap<String, String>) -> Self {
        Self { includes, defines }
    }
}

pub trait Validator {
    fn validate_shader(
        &mut self,
        shader_content: String,
        file_path: &Path,
        params: ValidationParams,
    ) -> Result<(ShaderDiagnosticList, Dependencies), ValidatorError>;

    // Get shader completion should not be part of validator.
    // It should only handle validation. Using the lib for that requires the
    // shader to compile, which might not be the case when typing.
    // Symbol DB can be managed through regex only
    fn get_shader_completion(
        &mut self,
        shader_content: String,
        file_path: &Path,
        params: ValidationParams,
    ) -> Result<ShaderSymbolList, ValidatorError>;

    fn get_file_name(&self, path: &Path) -> String {
        String::from(path.file_name().unwrap_or_default().to_string_lossy())
    }
}