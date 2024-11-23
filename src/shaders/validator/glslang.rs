use super::validator::{ValidationParams, Validator};
use crate::shaders::{
    include::{Dependencies, IncludeHandler},
    shader::{GlslSpirvVersion, GlslTargetClient, ShaderStage},
    shader_error::{
        ShaderDiagnostic, ShaderDiagnosticList, ShaderError, ShaderErrorSeverity, ValidatorError,
    },
};
use glslang::{
    error::GlslangError,
    include::{IncludeResult, IncludeType},
    Compiler, CompilerOptions, ShaderInput, ShaderSource,
};
use std::{
    borrow::Borrow,
    path::{Path, PathBuf},
};

impl Into<glslang::ShaderStage> for ShaderStage {
    fn into(self) -> glslang::ShaderStage {
        match self {
            ShaderStage::Vertex => glslang::ShaderStage::Vertex,
            ShaderStage::Fragment => glslang::ShaderStage::Fragment,
            ShaderStage::Compute => glslang::ShaderStage::Compute,
            ShaderStage::TesselationControl => glslang::ShaderStage::TesselationControl,
            ShaderStage::TesselationEvaluation => glslang::ShaderStage::TesselationEvaluation,
            ShaderStage::Mesh => glslang::ShaderStage::Mesh,
            ShaderStage::Task => glslang::ShaderStage::Task,
            ShaderStage::Geometry => glslang::ShaderStage::Geometry,
            ShaderStage::RayGeneration => glslang::ShaderStage::RayGeneration,
            ShaderStage::ClosestHit => glslang::ShaderStage::ClosestHit,
            ShaderStage::AnyHit => glslang::ShaderStage::AnyHit,
            ShaderStage::Callable => glslang::ShaderStage::Callable,
            ShaderStage::Miss => glslang::ShaderStage::Miss,
            ShaderStage::Intersect => glslang::ShaderStage::Intersect,
        }
    }
}

pub struct Glslang {
    hlsl: bool,
    compiler: &'static Compiler,
}

impl Glslang {
    #[allow(dead_code)] // Only used for WASI (alternative to DXC)
    pub fn hlsl() -> Self {
        let compiler = Compiler::acquire().expect("Failed to create glslang compiler");
        Self {
            hlsl: true,
            compiler,
        }
    }
    pub fn glsl() -> Self {
        let compiler = Compiler::acquire().expect("Failed to create glslang compiler");
        Self {
            hlsl: false,
            compiler,
        }
    }
}

struct GlslangIncludeHandler<'a> {
    include_handler: IncludeHandler,
    content: Option<&'a String>,
    file_name: &'a Path,
    include_callback: &'a mut dyn FnMut(&Path) -> Option<String>,
}

impl<'a> GlslangIncludeHandler<'a> {
    pub fn new(
        file: &'a Path,
        includes: Vec<String>,
        content: Option<&'a String>,
        include_callback: &'a mut dyn FnMut(&Path) -> Option<String>,
    ) -> Self {
        Self {
            include_handler: IncludeHandler::new(file, includes),
            content: content,
            file_name: file,
            include_callback: include_callback,
        }
    }
    pub fn get_dependencies(&self) -> &Dependencies {
        self.include_handler.get_dependencies()
    }
}

impl glslang::include::IncludeHandler for GlslangIncludeHandler<'_> {
    fn include(
        &mut self,
        _ty: IncludeType, // TODO: should use them ?
        header_name: &str,
        includer_name: &str,
        _include_depth: usize,
    ) -> Option<IncludeResult> {
        if Path::new(header_name) == self.file_name {
            match self.content {
                Some(value) => {
                    return Some(IncludeResult {
                        name: String::from(header_name),
                        data: value.clone(),
                    })
                }
                None => {}
            }
        }
        let filename = if includer_name.is_empty() {
            PathBuf::from(header_name)
        } else {
            if let Some(parent) = Path::new(includer_name).parent() {
                parent.join(header_name)
            } else {
                PathBuf::from(header_name)
            }
        };
        match self
            .include_handler
            .search_in_includes(filename.as_path(), self.include_callback)
        {
            Some(data) => Some(IncludeResult {
                name: String::from(header_name),
                data: data.0,
            }),
            None => None,
        }
    }
}

// GLSLang does not support linting header file, so to lint them,
// We include them in a template file.
const INCLUDE_RESOLVING: &str = r#"
#version 450
#extension GL_GOOGLE_include_directive : require
#include "{}"
void main() {
}
"#;

impl Glslang {
    fn parse_errors(
        errors: &String,
        file_path: &Path,
        includes: &Vec<String>,
    ) -> Result<ShaderDiagnosticList, ValidatorError> {
        let mut shader_error_list = ShaderDiagnosticList::empty();

        let reg = regex::Regex::new(r"(?m)^(.*?:(?:  \d+:\d+:)?)")?;
        let mut starts = Vec::new();
        for capture in reg.captures_iter(errors.as_str()) {
            if let Some(pos) = capture.get(0) {
                starts.push(pos.start());
            }
        }
        starts.push(errors.len());
        let internal_reg = regex::Regex::new(
            r"(?s)^(.*?):(?: ((?:[a-zA-Z]:)?[\d\w\.\/\\\-]+):(\d+):(\d+):)?(.+)",
        )?;
        let mut include_handler = IncludeHandler::new(file_path, includes.clone());
        for start in 0..starts.len() - 1 {
            let first = starts[start];
            let length = starts[start + 1] - starts[start];
            let block: String = errors.chars().skip(first).take(length).collect();
            if block.contains("compilation errors.  No code generated.") {
                continue; // Skip this useless string.
            }
            if let Some(capture) = internal_reg.captures(block.as_str()) {
                let level = capture.get(1).map_or("", |m| m.as_str());
                let relative_path = capture.get(2).map_or("", |m| m.as_str());
                let line = capture.get(3).map_or("", |m| m.as_str());
                let pos = capture.get(4).map_or("", |m| m.as_str());
                let msg = capture.get(5).map_or("", |m| m.as_str());
                shader_error_list.push(ShaderDiagnostic {
                    file_path: match relative_path.parse::<u32>() {
                        Ok(_) => None, // Main file
                        Err(_) => {
                            if relative_path.is_empty() {
                                None
                            } else {
                                include_handler.search_path_in_includes(Path::new(relative_path))
                            }
                        }
                    },
                    severity: match level {
                        "ERROR" => ShaderErrorSeverity::Error,
                        "WARNING" => ShaderErrorSeverity::Warning,
                        "NOTE" => ShaderErrorSeverity::Information,
                        "HINT" => ShaderErrorSeverity::Hint,
                        _ => ShaderErrorSeverity::Error,
                    },
                    error: String::from(msg),
                    line: line.parse::<u32>().unwrap_or(1),
                    pos: pos.parse::<u32>().unwrap_or(0),
                });
            } else {
                return Err(ValidatorError::internal(format!(
                    "Failed to parse regex: {}",
                    block
                )));
            }
        }

        if shader_error_list.is_empty() {
            return Err(ValidatorError::internal(format!(
                "Failed to parse errors: {}",
                errors
            )));
        }
        return Ok(shader_error_list);
    }

    fn from_glslang_error(
        &self,
        err: GlslangError,
        file_path: &Path,
        params: &ValidationParams,
    ) -> ShaderError {
        match err {
            GlslangError::PreprocessError(error) => {
                match Glslang::parse_errors(&error, file_path, &params.includes) {
                    Ok(diag) => ShaderError::DiagnosticList(diag),
                    Err(err) => ShaderError::Validator(err),
                }
            }
            GlslangError::ParseError(error) => {
                match Glslang::parse_errors(&error, file_path, &params.includes) {
                    Ok(diag) => ShaderError::DiagnosticList(diag),
                    Err(err) => ShaderError::Validator(err),
                }
            }
            GlslangError::LinkError(error) => {
                match Glslang::parse_errors(&error, file_path, &params.includes) {
                    Ok(diag) => ShaderError::DiagnosticList(diag),
                    Err(err) => ShaderError::Validator(err),
                }
            }
            GlslangError::ShaderStageNotFound(stage) => ShaderError::Validator(
                ValidatorError::internal(format!("Shader stage not found: {:#?}", stage)),
            ),
            GlslangError::InvalidProfile(target, value, profile) => {
                ShaderError::Validator(ValidatorError::internal(format!(
                    "Invalid profile {} for target {:#?}: {:#?}",
                    value, target, profile
                )))
            }
            GlslangError::VersionUnsupported(value, profile) => ShaderError::Validator(
                ValidatorError::internal(format!("Unsupported profile {}: {:#?}", value, profile)),
            ),
            err => ShaderError::Validator(ValidatorError::internal(format!(
                "Internal error: {:#?}",
                err
            ))),
        }
    }
}
impl Validator for Glslang {
    fn validate_shader(
        &mut self,
        content: String,
        file_path: &Path,
        params: ValidationParams,
        include_callback: &mut dyn FnMut(&Path) -> Option<String>,
    ) -> Result<(ShaderDiagnosticList, Dependencies), ValidatorError> {
        let file_name = self.get_file_name(file_path);

        let (shader_stage, shader_source) =
            if let Some(shader_stage) = ShaderStage::from_file_name(&file_name) {
                (shader_stage, content.clone())
            } else {
                // If we dont have a stage, treat it as an include by including it in template file.
                // GLSLang requires to have stage for linting.
                // This will prevent lint on typing to works though... except if we use callback
                (
                    ShaderStage::Fragment,
                    INCLUDE_RESOLVING.replace("{}", file_path.to_string_lossy().borrow()),
                )
            };

        let source = ShaderSource::try_from(shader_source).expect("Failed to read from source");

        let defines_copy = params.defines.clone();
        let defines: Vec<(&str, Option<&str>)> = defines_copy
            .iter()
            .map(|v| (&v.0 as &str, Some(&v.1 as &str)))
            .collect();
        let mut include_handler = GlslangIncludeHandler::new(
            file_path,
            params.includes.clone(),
            Some(&content),
            include_callback,
        );

        let lang_version = match params.glsl_spirv {
            GlslSpirvVersion::SPIRV1_0 => glslang::SpirvVersion::SPIRV1_0,
            GlslSpirvVersion::SPIRV1_1 => glslang::SpirvVersion::SPIRV1_1,
            GlslSpirvVersion::SPIRV1_2 => glslang::SpirvVersion::SPIRV1_2,
            GlslSpirvVersion::SPIRV1_3 => glslang::SpirvVersion::SPIRV1_3,
            GlslSpirvVersion::SPIRV1_4 => glslang::SpirvVersion::SPIRV1_4,
            GlslSpirvVersion::SPIRV1_5 => glslang::SpirvVersion::SPIRV1_5,
            GlslSpirvVersion::SPIRV1_6 => glslang::SpirvVersion::SPIRV1_6,
        };
        let input = match ShaderInput::new(
            &source,
            shader_stage.into(),
            &CompilerOptions {
                source_language: if self.hlsl {
                    glslang::SourceLanguage::HLSL
                } else {
                    glslang::SourceLanguage::GLSL
                },
                // Should have some settings to select these.
                target: if self.hlsl {
                    glslang::Target::None(Some(lang_version))
                } else {
                    if params.glsl_client.is_opengl() {
                        glslang::Target::OpenGL {
                            version: glslang::OpenGlVersion::OpenGL4_5,
                            spirv_version: None, // TODO ?
                        }
                    } else {
                        let client_version = match params.glsl_client {
                            GlslTargetClient::Vulkan1_0 => glslang::VulkanVersion::Vulkan1_0,
                            GlslTargetClient::Vulkan1_1 => glslang::VulkanVersion::Vulkan1_1,
                            GlslTargetClient::Vulkan1_2 => glslang::VulkanVersion::Vulkan1_2,
                            GlslTargetClient::Vulkan1_3 => glslang::VulkanVersion::Vulkan1_3,
                            _ => unreachable!(),
                        };
                        glslang::Target::Vulkan {
                            version: client_version,
                            spirv_version: lang_version,
                        }
                    }
                },
                messages: glslang::ShaderMessage::CASCADING_ERRORS
                    | glslang::ShaderMessage::DEBUG_INFO
                    | glslang::ShaderMessage::DISPLAY_ERROR_COLUMN,
                ..Default::default()
            },
            Some(&defines),
            Some(&mut include_handler),
        )
        .map_err(|e| self.from_glslang_error(e, file_path, &params))
        {
            Ok(value) => value,
            Err(error) => match error {
                ShaderError::Validator(error) => return Err(error),
                ShaderError::DiagnosticList(diag) => {
                    return Ok((diag, include_handler.get_dependencies().clone()))
                }
            },
        };
        let _shader = match glslang::Shader::new(&self.compiler, input)
            .map_err(|e| self.from_glslang_error(e, file_path, &params))
        {
            Ok(value) => value,
            Err(error) => match error {
                ShaderError::Validator(error) => return Err(error),
                ShaderError::DiagnosticList(diag) => {
                    return Ok((diag, include_handler.get_dependencies().clone()))
                }
            },
        };
        // Linking require main entry point. Should work around this somehow.
        /*let _spirv = match shader.compile().map_err(|e| self.from_glslang_error(e)) {
            Ok(value) => value,
            Err(error) => match error {
                ShaderError::Validator(error) => return Err(error),
                ShaderError::DiagnosticList(diag) => return Ok((diag, include_handler.get_dependencies().clone())),
            },
        };*/

        Ok((
            ShaderDiagnosticList::empty(),
            include_handler.get_dependencies().clone(),
        )) // No error detected.
    }
}
