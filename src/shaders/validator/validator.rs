use std::{collections::HashMap, path::Path};

use crate::shaders::{
    include::Dependencies,
    shader_error::{ShaderDiagnosticList, ValidatorError},
};

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

    fn get_file_name(&self, path: &Path) -> String {
        String::from(path.file_name().unwrap_or_default().to_string_lossy())
    }
}
