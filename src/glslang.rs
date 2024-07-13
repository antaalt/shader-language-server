use std::path::Path;
use crate::{common::{ShaderTree, ValidationParams, Validator}, shader_error::{ShaderError, ShaderErrorList, ShaderErrorSeverity}};
use glslang::{error::GlslangError, Compiler, CompilerOptions, ShaderInput, ShaderSource};
use glslang::*;
use include::{IncludeResult, IncludeType};

impl From<regex::Error> for ShaderErrorList {
    fn from(error: regex::Error) -> Self {
        match error {
            regex::Error::CompiledTooBig(err) => ShaderErrorList::internal(format!("Regex compile too big: {}", err)),
            regex::Error::Syntax(err) => ShaderErrorList::internal(format!("Regex syntax invalid: {}", err)),
            err =>  ShaderErrorList::internal(format!("Regex error: {:#?}", err))
        }
    }
}

pub struct Glslang {
    hlsl: bool
}

impl Glslang {
    #[allow(dead_code)] // Only used for WASI (alternative to DXC)
    pub fn hlsl() -> Self {
        Self {
            hlsl: true
        }
    }
    pub fn glsl() -> Self {
        Self {
            hlsl: false
        }
    }
}


fn include_handler(_t: IncludeType, _p: &str, _p2: &str, _s: usize) -> Option<IncludeResult>
{
    // We cant add custom include path here.... 
    // We have include type
    // p which is include path
    // P2 i dont know
    // s which is i dont know either...
    None
}

impl From<GlslangError> for ShaderErrorList {
    fn from(err: GlslangError) -> Self {
        match err {
            GlslangError::PreprocessError(error) => {
                match Glslang::parse_errors(&error) {
                    Ok(err) => err,
                    Err(err) => err
                }
            },
            GlslangError::ParseError(error) => {
                match Glslang::parse_errors(&error) {
                    Ok(err) => err,
                    Err(err) => err
                }
            },
            GlslangError::LinkError(error) => {
                match Glslang::parse_errors(&error) {
                    Ok(err) => err,
                    Err(err) => err
                }
            },
            GlslangError::ShaderStageNotFound(stage) => {
                ShaderErrorList::from(ShaderError::ValidationErr{ src: String::from(""), emitted: format!("Shader stage not found: {:#?}", stage)})
            },
            GlslangError::InvalidProfile(target, value, profile) => {
                ShaderErrorList::internal(format!("Invalid profile {} for target {:#?}: {:#?}", value, target, profile))
            },
            GlslangError::VersionUnsupported(value, profile) => {
                ShaderErrorList::internal(format!("Unsupported profile {}: {:#?}", value, profile))
            },
            err => ShaderErrorList::internal(format!("Internal error: {:#?}", err))
        }
    }
}

impl Glslang {
    fn parse_errors(errors: &String) -> Result<ShaderErrorList, ShaderErrorList>
    {
        let mut shader_error_list = ShaderErrorList::empty();

        let reg = regex::Regex::new(r"(?m)^(.*?: \d+:\d+:)")?;
        let mut starts = Vec::new();
        for capture in reg.captures_iter(errors.as_str()) {
            starts.push(capture.get(0).unwrap().start());
        }
        starts.push(errors.len());
        let internal_reg = regex::Regex::new(r"(?m)^(.*?): (\d+):(\d+):(.+)")?;
        for start in 0..starts.len()-1 {
            let first = starts[start];
            let length = starts[start + 1] - starts[start];
            let block : String = errors.chars().skip(first).take(length).collect();
            if let Some(capture) = internal_reg.captures(block.as_str()) {
                let level = capture.get(1).map_or("", |m| m.as_str());
                // First number is not pos.
                // https://github.com/KhronosGroup/glslang/issues/3238
                let _str = capture.get(2).map_or("", |m| m.as_str());
                let line = capture.get(3).map_or("", |m| m.as_str());
                // Add position once following PR is merged
                // https://github.com/KhronosGroup/glslang/pull/3614
                //let pos = capture.get(4).map_or("", |m| m.as_str());
                let msg = capture.get(4).map_or("", |m| m.as_str());
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
                    line: line.parse::<usize>().unwrap_or(1),
                    pos: 0//pos.parse::<usize>().unwrap_or(0),
                });
            }
            else 
            {
                shader_error_list.push(ShaderError::InternalErr(format!("Failed to parse regex: {}", block)));
            }
        }
        return Ok(shader_error_list);
    }
}
impl Validator for Glslang {
    fn validate_shader(&mut self, path: &Path, cwd: &Path, _params: ValidationParams) -> Result<(), ShaderErrorList> {
        let shader_string = std::fs::read_to_string(&path)?;

        let compiler = Compiler::acquire().unwrap();
        let source = ShaderSource::try_from(shader_string).expect("Failed to read from source");

        let input = ShaderInput::new(
            &source,
            ShaderStage::Fragment,
            &CompilerOptions {
                source_language: if self.hlsl { SourceLanguage::HLSL } else { SourceLanguage::GLSL },
                // Should have some settings to select these.
                target: if self.hlsl {
                    Target::None(Some(SpirvVersion::SPIRV1_6))
                } else {
                    Target::Vulkan { 
                        version: VulkanVersion::Vulkan1_3, 
                        spirv_version: SpirvVersion::SPIRV1_6 
                    }
                },
                messages: ShaderMessage::CASCADING_ERRORS | ShaderMessage::DEBUG_INFO,
                ..Default::default()
            },
            Some(include_handler), // TODO: need access to include system callback to pass custom pass.
        )?;
        let _shader = Shader::new(&compiler, input)?;
        // TODO: Cannot add macro in glslang currently. The only way is through preamble and glslang-rs does not allow access.
        /*let preamble = String::new();
        for define in _params.defines
        {
            preamble += format!("{} {}\n", define.0, define.1).as_str();
        }
        _shader.preamble(preamble);*/
        
        Ok(())
    }

    fn get_shader_tree(&mut self, path: &Path, cwd: &Path, _params: ValidationParams) -> Result<ShaderTree, ShaderErrorList> {
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
