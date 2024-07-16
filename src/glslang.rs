use std::path::Path;
use crate::{common::{ShaderTree, ValidationParams, Validator}, shader_error::{ShaderError, ShaderErrorList, ShaderErrorSeverity}};
use glslang::{error::GlslangError, Compiler, CompilerOptions, ShaderInput, ShaderSource};
use glslang::*;
use include::{IncludeHandler, IncludeResult, IncludeType};

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
    hlsl: bool,
    compiler: &'static Compiler,
}

impl Glslang {
    #[allow(dead_code)] // Only used for WASI (alternative to DXC)
    pub fn hlsl() -> Self {
        let compiler = Compiler::acquire().unwrap();
        Self {
            hlsl: true,
            compiler,
        }
    }
    pub fn glsl() -> Self {
        let compiler = Compiler::acquire().unwrap();
        Self {
            hlsl: false,
            compiler,
        }
    }
}

struct GlslangIncludeHandler {
    includes: Vec<String>
}

impl IncludeHandler for GlslangIncludeHandler {
    fn include(&self, _ty: IncludeType, header_name: &str, _includer_name : &str, _include_depth : usize) -> Option<IncludeResult> {
        let path = Path::new(&header_name);
        if path.exists() {
            if let Some(data) = self.read(&path) {
                Some(IncludeResult {
                    name: String::from(header_name),
                    data: data
                })
            } else {
                None
            }
        } else {
            for include in &self.includes {
                let path = Path::new(include).join(&header_name);
                let content = self.read(&path);
                if let Some(data) = content {
                    return Some(IncludeResult {
                        name: String::from(header_name),
                        data: data
                    });
                }
            }
            None
        }
    }
}
impl GlslangIncludeHandler {
    pub fn new(cwd: &Path, params: ValidationParams) -> Self {
        // Add local path to include path
        let mut includes = params.includes;
        let str = String::from(cwd.to_string_lossy());
        includes.push(str);
        Self {
            includes,
        }
    }
    pub fn read(&self, path: &Path) -> Option<String> {
        use std::io::Read;
        match std::fs::File::open(path) {
            Ok(mut f) => {
                let mut content = String::new();
                f.read_to_string(&mut content).ok()?;
                Some(content)
            }
            Err(_) => None,
        }
    }
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
                ShaderErrorList::from(ShaderError::ValidationErr{ message: format!("Shader stage not found: {:#?}", stage)})
            },
            GlslangError::InvalidProfile(target, value, profile) => {
                ShaderErrorList::from(ShaderError::ValidationErr{ message: format!("Invalid profile {} for target {:#?}: {:#?}", value, target, profile)})
            },
            GlslangError::VersionUnsupported(value, profile) => {
                ShaderErrorList::from(ShaderError::ValidationErr{ message: format!("Unsupported profile {}: {:#?}", value, profile)})
            },
            err => ShaderErrorList::internal(format!("Internal error: {:#?}", err))
        }
    }
}

impl Glslang {
    fn parse_errors(errors: &String) -> Result<ShaderErrorList, ShaderErrorList>
    {
        let mut shader_error_list = ShaderErrorList::empty();

        let reg = regex::Regex::new(r"(?m)^(.*?:(?:  \d+:\d+:)?)")?;
        let mut starts = Vec::new();
        for capture in reg.captures_iter(errors.as_str()) {
            starts.push(capture.get(0).unwrap().start());
        }
        starts.push(errors.len());
        let internal_reg = regex::Regex::new(r"(?s)^(.*?):(?: (\d+):(\d+):)?(.+)")?;
        for start in 0..starts.len()-1 {
            let first = starts[start];
            let length = starts[start + 1] - starts[start];
            let block : String = errors.chars().skip(first).take(length).collect();
            if block.contains("compilation errors.  No code generated.") {
                continue; // Skip this useless string.
            }
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
        
        if shader_error_list.errors.len() == 0 {
            shader_error_list.push(ShaderError::InternalErr(format!("Failed to parse errors: {}", errors)));
        }
        return Ok(shader_error_list);
    }
}
impl Validator for Glslang {
    fn validate_shader(&mut self, path: &Path, cwd: &Path, params: ValidationParams) -> Result<(), ShaderErrorList> {
        let shader_string = std::fs::read_to_string(&path)?;

        let source = ShaderSource::try_from(shader_string).expect("Failed to read from source");
        
        let defines_copy = params.defines.clone();
        let defines : Vec<(&str, Option<&str>)> = defines_copy.iter().map(|v| (&v.0 as &str, Some(&v.1 as &str))).collect();
        let mut include_handler = GlslangIncludeHandler::new(cwd, params);
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
            &defines,
            Some(&mut include_handler),
        )?;
        let _shader = Shader::new(&self.compiler, input)?;
        
        Ok(())
    }

    fn get_shader_tree(&mut self, path: &Path, _cwd: &Path, _params: ValidationParams) -> Result<ShaderTree, ShaderErrorList> {
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
