use std::{collections::HashMap, path::Path};

use crate::shaders::{
    include::Dependencies,
    shader::{GlslSpirvVersion, GlslTargetClient, HlslShaderModel},
    shader_error::{ShaderDiagnosticList, ValidatorError},
};

pub struct ValidationParams {
    pub includes: Vec<String>,
    pub defines: HashMap<String, String>,
    pub hlsl_shader_model: HlslShaderModel,
    pub glsl_client: GlslTargetClient,
    pub glsl_spirv: GlslSpirvVersion,
}

impl ValidationParams {
    pub fn new(includes: Vec<String>, defines: HashMap<String, String>) -> Self {
        Self {
            includes,
            defines,
            hlsl_shader_model: HlslShaderModel::ShaderModel6_8,
            glsl_client: GlslTargetClient::Vulkan1_3,
            glsl_spirv: GlslSpirvVersion::SPIRV1_6,
        }
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
