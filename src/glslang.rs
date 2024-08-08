use crate::{
    common::{ShaderTree, ValidationParams, Validator},
    include::IncludeHandler,
    shader_error::{ShaderError, ShaderErrorList, ShaderErrorSeverity},
};
use glslang::*;
use glslang::{error::GlslangError, Compiler, CompilerOptions, ShaderInput, ShaderSource};
use include::{IncludeResult, IncludeType};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

impl From<regex::Error> for ShaderErrorList {
    fn from(error: regex::Error) -> Self {
        match error {
            regex::Error::CompiledTooBig(err) => {
                ShaderErrorList::internal(format!("Regex compile too big: {}", err))
            }
            regex::Error::Syntax(err) => {
                ShaderErrorList::internal(format!("Regex syntax invalid: {}", err))
            }
            err => ShaderErrorList::internal(format!("Regex error: {:#?}", err)),
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

impl glslang::include::IncludeHandler for IncludeHandler {
    fn include(
        &self,
        _ty: IncludeType, // TODO: should use them ?
        header_name: &str,
        includer_name: &str,
        _include_depth: usize,
    ) -> Option<IncludeResult> {
        let filename = if includer_name.is_empty() {
            PathBuf::from(header_name)
        } else {
            if let Some(parent) = Path::new(includer_name).parent() {
                parent.join(header_name)
            } else {
                PathBuf::from(header_name)
            }
        };
        match self.search_in_includes(filename.as_path()) {
            Some(data) => Some(IncludeResult {
                name: String::from(header_name),
                data: data,
            }),
            None => None,
        }
    }
}

impl From<GlslangError> for ShaderErrorList {
    fn from(err: GlslangError) -> Self {
        match err {
            GlslangError::PreprocessError(error) => match Glslang::parse_errors(&error) {
                Ok(err) => err,
                Err(err) => err,
            },
            GlslangError::ParseError(error) => match Glslang::parse_errors(&error) {
                Ok(err) => err,
                Err(err) => err,
            },
            GlslangError::LinkError(error) => match Glslang::parse_errors(&error) {
                Ok(err) => err,
                Err(err) => err,
            },
            GlslangError::ShaderStageNotFound(stage) => {
                ShaderErrorList::from(ShaderError::ValidationErr {
                    message: format!("Shader stage not found: {:#?}", stage),
                })
            }
            GlslangError::InvalidProfile(target, value, profile) => {
                ShaderErrorList::from(ShaderError::ValidationErr {
                    message: format!(
                        "Invalid profile {} for target {:#?}: {:#?}",
                        value, target, profile
                    ),
                })
            }
            GlslangError::VersionUnsupported(value, profile) => {
                ShaderErrorList::from(ShaderError::ValidationErr {
                    message: format!("Unsupported profile {}: {:#?}", value, profile),
                })
            }
            err => ShaderErrorList::internal(format!("Internal error: {:#?}", err)),
        }
    }
}

impl Glslang {
    fn parse_errors(errors: &String) -> Result<ShaderErrorList, ShaderErrorList> {
        let mut shader_error_list = ShaderErrorList::empty();

        let reg = regex::Regex::new(r"(?m)^(.*?:(?:  \d+:\d+:)?)")?;
        let mut starts = Vec::new();
        for capture in reg.captures_iter(errors.as_str()) {
            if let Some(pos) = capture.get(0) {
                starts.push(pos.start());
            }
        }
        starts.push(errors.len());
        let internal_reg = regex::Regex::new(r"(?s)^(.*?):(?: (\d+):(\d+):(\d+):)?(.+)")?;
        for start in 0..starts.len() - 1 {
            let first = starts[start];
            let length = starts[start + 1] - starts[start];
            let block: String = errors.chars().skip(first).take(length).collect();
            if block.contains("compilation errors.  No code generated.") {
                continue; // Skip this useless string.
            }
            if let Some(capture) = internal_reg.captures(block.as_str()) {
                let level = capture.get(1).map_or("", |m| m.as_str());
                let _str = capture.get(2).map_or("", |m| m.as_str());
                let line = capture.get(3).map_or("", |m| m.as_str());
                let pos = capture.get(4).map_or("", |m| m.as_str());
                let msg = capture.get(5).map_or("", |m| m.as_str());
                shader_error_list.push(ShaderError::ParserErr {
                    filename: None, // TODO: Could get it from logs.
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
                shader_error_list.push(ShaderError::InternalErr(format!(
                    "Failed to parse regex: {}",
                    block
                )));
            }
        }

        if shader_error_list.errors.len() == 0 {
            shader_error_list.push(ShaderError::InternalErr(format!(
                "Failed to parse errors: {}",
                errors
            )));
        }
        return Ok(shader_error_list);
    }

    // GLSLang requires a stage to be passed, so pick one depending on extension.
    // If none is found, use a default one.
    fn get_shader_stage_from_filename(&self, file_name: &String) -> ShaderStage {
        // TODO: add control for these
        let paths = HashMap::from([
            ("vert", ShaderStage::Vertex),
            ("frag", ShaderStage::Fragment),
            ("comp", ShaderStage::Compute),
            ("task", ShaderStage::Task),
            ("mesh", ShaderStage::Mesh),
            ("tesc", ShaderStage::TesselationControl),
            ("tese", ShaderStage::TesselationEvaluation),
            ("geom", ShaderStage::Geometry),
            ("rgen", ShaderStage::RayGeneration),
            ("rchit", ShaderStage::ClosestHit),
            ("rahit", ShaderStage::AnyHit),
            ("rcall", ShaderStage::Callable),
            ("rmiss", ShaderStage::Miss),
            ("rint", ShaderStage::Intersect),
        ]);
        let extension_list = file_name.rsplit(".");
        for extension in extension_list {
            if let Some(stage) = paths.get(extension) {
                return stage.clone();
            } else {
                continue;
            }
        }
        // For header files & undefined, will output issue with missing version...
        // Could have a default value
        ShaderStage::Fragment
    }
}
impl Validator for Glslang {
    fn validate_shader(
        &mut self,
        shader_source: String,
        filename: String,
        cwd: &Path,
        params: ValidationParams,
    ) -> Result<(), ShaderErrorList> {

        let source = ShaderSource::try_from(shader_source).expect("Failed to read from source");

        let defines_copy = params.defines.clone();
        let defines: Vec<(&str, Option<&str>)> = defines_copy
            .iter()
            .map(|v| (&v.0 as &str, Some(&v.1 as &str)))
            .collect();
        let mut include_handler = IncludeHandler::new(cwd, params.includes);
        let input = ShaderInput::new(
            &source,
            self.get_shader_stage_from_filename(&filename),
            &CompilerOptions {
                source_language: if self.hlsl {
                    SourceLanguage::HLSL
                } else {
                    SourceLanguage::GLSL
                },
                // Should have some settings to select these.
                target: if self.hlsl {
                    Target::None(Some(SpirvVersion::SPIRV1_6))
                } else {
                    Target::Vulkan {
                        version: VulkanVersion::Vulkan1_3,
                        spirv_version: SpirvVersion::SPIRV1_6,
                    }
                },
                messages: ShaderMessage::CASCADING_ERRORS
                    | ShaderMessage::DEBUG_INFO
                    | ShaderMessage::DISPLAY_ERROR_COLUMN,
                ..Default::default()
            },
            &defines,
            Some(&mut include_handler),
        )?;
        let _shader = Shader::new(&self.compiler, input)?;

        Ok(())
    }

    fn get_shader_tree(
        &mut self,
        path: &Path,
        _cwd: &Path,
        _params: ValidationParams,
    ) -> Result<ShaderTree, ShaderErrorList> {
        let _shader = std::fs::read_to_string(&path).map_err(ShaderErrorList::from)?;
        let types = Vec::new();
        let global_variables = Vec::new();
        let functions = Vec::new();

        Ok(ShaderTree {
            types,
            global_variables,
            functions,
        })
    }
}
