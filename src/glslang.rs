use crate::{
    common::{
        get_default_shader_completion, get_shader_position, ShaderParameter, ShaderPosition, ShaderSignature, ShaderStage, ShaderSymbol, ShaderSymbolList, ShadingLanguage, ValidationParams, Validator
    },
    include::{Dependencies, IncludeHandler},
    shader_error::{
        ShaderDiagnostic, ShaderDiagnosticList, ShaderError, ShaderErrorSeverity, ValidatorError,
    },
};
use glslang::{
    error::GlslangError,
    include::{IncludeResult, IncludeType},
    Compiler, CompilerOptions, ShaderInput, ShaderSource,
};
use log::{error, warn};
use regex::Regex;
use std::{
    borrow::Borrow, cmp::Ordering, collections::HashMap, path::{Path, PathBuf}
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
}

impl<'a> GlslangIncludeHandler<'a> {
    pub fn new(file: &'a Path, includes: Vec<String>, content: Option<&'a String>) -> Self {
        Self {
            include_handler: IncludeHandler::new(file, includes),
            content: content,
            file_name: file,
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
        error!("OUI::: {} / {}", header_name, self.file_name.display());
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
        match self.include_handler.search_in_includes(filename.as_path()) {
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
    fn parse_errors(errors: &String) -> Result<ShaderDiagnosticList, ValidatorError> {
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
        for start in 0..starts.len() - 1 {
            let first = starts[start];
            let length = starts[start + 1] - starts[start];
            let block: String = errors.chars().skip(first).take(length).collect();
            if block.contains("compilation errors.  No code generated.") {
                continue; // Skip this useless string.
            }
            if let Some(capture) = internal_reg.captures(block.as_str()) {
                let level = capture.get(1).map_or("", |m| m.as_str());
                let file = capture.get(2).map_or("", |m| m.as_str());
                let line = capture.get(3).map_or("", |m| m.as_str());
                let pos = capture.get(4).map_or("", |m| m.as_str());
                let msg = capture.get(5).map_or("", |m| m.as_str());
                shader_error_list.push(ShaderDiagnostic {
                    relative_path: match file.parse::<u32>() {
                        Ok(_) => None, // Main file
                        Err(_) => {
                            if file.is_empty() {
                                None
                            } else {
                                Some(PathBuf::from(file))
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

    fn from_glslang_error(&self, err: GlslangError) -> ShaderError {
        match err {
            GlslangError::PreprocessError(error) => match Glslang::parse_errors(&error) {
                Ok(diag) => ShaderError::DiagnosticList(diag),
                Err(err) => ShaderError::Validator(err),
            },
            GlslangError::ParseError(error) => match Glslang::parse_errors(&error) {
                Ok(diag) => ShaderError::DiagnosticList(diag),
                Err(err) => ShaderError::Validator(err),
            },
            GlslangError::LinkError(error) => match Glslang::parse_errors(&error) {
                Ok(diag) => ShaderError::DiagnosticList(diag),
                Err(err) => ShaderError::Validator(err),
            },
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

    // GLSLang requires a stage to be passed, so pick one depending on extension.
    // If none is found, use a default one.
    fn get_shader_stage_from_filename(&self, file_name: &String) -> Option<ShaderStage> {
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
                return Some(stage.clone());
            } else {
                continue;
            }
        }
        // For header files & undefined, will output issue with missing version...
        None
    }
    fn filter_version(&self, _items: &mut ShaderSymbolList) {
        // TODO: read version from settings & filter completion items based on it.
    }
}
impl Validator for Glslang {
    fn validate_shader(
        &mut self,
        content: String,
        file_path: &Path,
        params: ValidationParams,
    ) -> Result<(ShaderDiagnosticList, Dependencies), ValidatorError> {
        let file_name = self.get_file_name(file_path);

        let (shader_stage, shader_source) =
            if let Some(shader_stage) = self.get_shader_stage_from_filename(&file_name) {
                (shader_stage, content.clone())
            } else {
                // If we dont have a stage, treat it as an include by including it in template file.
                // GLSLang requires to have stage for linting.
                // This will prevent lint on typing to works though...
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
        let mut include_handler =
            GlslangIncludeHandler::new(file_path, params.includes, Some(&content));
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
                    glslang::Target::None(Some(glslang::SpirvVersion::SPIRV1_6))
                } else {
                    glslang::Target::Vulkan {
                        version: glslang::VulkanVersion::Vulkan1_3,
                        spirv_version: glslang::SpirvVersion::SPIRV1_6,
                    }
                },
                messages: glslang::ShaderMessage::CASCADING_ERRORS
                    | glslang::ShaderMessage::DEBUG_INFO
                    | glslang::ShaderMessage::DISPLAY_ERROR_COLUMN,
                ..Default::default()
            },
            &defines,
            Some(&mut include_handler),
        )
        .map_err(|e| self.from_glslang_error(e))
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
            .map_err(|e| self.from_glslang_error(e))
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
    // TODO: rename get_shader_symbols & move out of validator.
    fn get_shader_completion(
        &mut self,
        shader_content: String,
        file_path: &Path,
        params: ValidationParams,
    ) -> Result<ShaderSymbolList, ValidatorError> {
        let file_name = self.get_file_name(file_path);

        // Get builtins
        let mut completion = get_default_shader_completion(ShadingLanguage::Glsl);
        if let Some(shader_stage) = self.get_shader_stage_from_filename(&file_name) {
            completion.filter_shader_completion(shader_stage);
        }
        self.filter_version(&mut completion);

        let mut handler = IncludeHandler::new(file_path, params.includes.clone());
        let mut dependencies = find_dependencies(&mut handler, &shader_content);
        dependencies.push((shader_content, file_path.into()));

        for (dependency_content, dependency_path) in dependencies {
            completion.types.append(&mut capture_struct_symbols(
                &dependency_content,
                &dependency_path,
            ));
            completion.functions.append(&mut capture_function_symbols(
                &dependency_content,
                &dependency_path,
            ));
            completion.variables.append(&mut capture_variable_symbols(
                &dependency_content,
                &dependency_path,
            ));
            completion.variables.append(&mut capture_macro_symbols(
                &dependency_content,
                &dependency_path,
            ));
        }

        Ok(completion)
    }
}

fn capture_function_symbols(shader_content: &String, path: &Path) -> Vec<ShaderSymbol> {
    // Find function declarations
    let mut functions_declarations = Vec::new();
    let reg =
        Regex::new("\\b([\\w_]*)\\s+([\\w_-]*)[\\s]*\\(([\\s\\w,-\\[\\]]*)\\)[\\s]*\\{").unwrap();
    for capture in reg.captures_iter(&shader_content) {
        let signature = capture.get(2).unwrap();
        let return_type = capture.get(1).unwrap().as_str();
        let function = capture.get(2).unwrap().as_str();
        let parameters: Vec<&str> = match capture.get(3) {
            Some(all_parameters) => {
                if all_parameters.is_empty() {
                    Vec::new()
                } else {
                    all_parameters.as_str().split(',').collect()
                }
            }
            None => Vec::new(),
        };
        let position = get_shader_position(&shader_content, signature.start(), path);

        warn!(
            "Captured: {} {:?} at line {}:{}",
            function, parameters, position.line, position.pos
        );

        functions_declarations.push(ShaderSymbol {
            label: function.into(),
            description: "".into(),
            version: "".into(),
            stages: Vec::new(),
            link: None,
            signature: Some(ShaderSignature {
                returnType: return_type.to_string(),
                description: "".into(),
                parameters: parameters
                    .iter()
                    .map(|parameter| {
                        let values: Vec<&str> = parameter.split_whitespace().collect();
                        ShaderParameter {
                            ty: (*values.first().unwrap_or(&"void")).into(),
                            label: (*values.last().unwrap_or(&"type")).into(),
                            description: "".into(),
                        }
                    })
                    .collect(),
            }),
            ty: None,
            position: Some(position),
        });
    }
    functions_declarations
}
fn capture_struct_symbols(shader_content: &String, path: &Path) -> Vec<ShaderSymbol> {
    // Find struct & types declarations
    let mut struct_declarations = Vec::new();
    let regex_struct = Regex::new("\\bstruct\\s+([\\w_-]+)\\s*\\{").unwrap();
    for capture in regex_struct.captures_iter(&shader_content) {
        let name = capture.get(1).unwrap();

        let position = get_shader_position(&shader_content, name.start(), path);
        struct_declarations.push(ShaderSymbol {
            label: name.as_str().into(),
            description: "".into(),
            version: "".into(),
            stages: Vec::new(),
            link: None,
            signature: None,
            ty: None,
            position: Some(position),
        });
    }
    struct_declarations
}
fn capture_macro_symbols(shader_content: &String, path: &Path) -> Vec<ShaderSymbol> {
    // Find variable declarations
    let mut macros_declarations = Vec::new();
    let regex_macro = Regex::new("\\#define\\s+([\\w\\-]+)").unwrap();

    for capture in regex_macro.captures_iter(&shader_content) {
        let value = capture.get(1).unwrap();

        let position = get_shader_position(&shader_content, value.start(), path);
        macros_declarations.push(ShaderSymbol {
            label: value.as_str().into(),
            description: "preprocessor macro".into(),
            version: "".into(),
            stages: Vec::new(),
            link: None,
            signature: None,
            ty: None,
            position: Some(position),
        });
    }
    macros_declarations
}
fn capture_variable_symbols(shader_content: &String, path: &Path) -> Vec<ShaderSymbol> {
    // Find variable declarations
    let mut variables_declarations = Vec::new();
    // TODO: handle multiple decl (ex: uint a, b, c;)
    let regex_variable = Regex::new("\\b([\\w_]*)\\s+([\\w_-]*)\\s*[;=][^=]").unwrap();

    for capture in regex_variable.captures_iter(&shader_content) {
        let ty = capture.get(1).unwrap();
        let name = capture.get(2).unwrap();

        let position = get_shader_position(&shader_content, name.start(), path);
        variables_declarations.push(ShaderSymbol {
            label: name.as_str().into(),
            description: "".into(),
            version: "".into(),
            stages: Vec::new(),
            link: None,
            signature: None,
            ty: Some(ty.as_str().into()),
            position: Some(position),
        });
    }
    variables_declarations
}
fn find_dependencies(
    include_handler: &mut IncludeHandler,
    shader_content: &String,
) -> Vec<(String, PathBuf)> {
    let include_regex = Regex::new("\\#include\\s+\"([\\w\\s\\\\/\\.\\-]+)\"").unwrap();
    let dependencies_paths: Vec<&str> = include_regex
        .captures_iter(&shader_content)
        .map(|c| c.get(1).unwrap().as_str())
        .collect();

    let mut dependencies = dependencies_paths
        .iter()
        .filter_map(|dependency| include_handler.search_in_includes(Path::new(dependency)))
        .collect::<Vec<(String, PathBuf)>>();

    let mut recursed_dependencies = Vec::new();
    for dependency in &dependencies {
        recursed_dependencies.append(&mut find_dependencies(include_handler, &dependency.0));
    }
    recursed_dependencies.append(&mut dependencies);

    recursed_dependencies
}
