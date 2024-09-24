use std::{collections::HashMap, path::Path};

use crate::shaders::{
    include::Dependencies,
    shader::{GlslSpirvVersion, GlslTargetClient, HlslShaderModel, HlslVersion},
    shader_error::{ShaderDiagnosticList, ValidatorError},
};

#[derive(Debug, Default)]
pub struct ValidationParams {
    pub includes: Vec<String>,
    pub defines: HashMap<String, String>,
    pub hlsl_shader_model: HlslShaderModel,
    pub hlsl_version: HlslVersion,
    pub hlsl_enable16bit_types: bool,
    pub glsl_client: GlslTargetClient,
    pub glsl_spirv: GlslSpirvVersion,
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
