use std::{
    cmp::Ordering,
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
};

use serde::{Deserialize, Serialize};

use crate::{
    include::Dependencies,
    shader_error::{ShaderDiagnosticList, ValidatorError},
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ShadingLanguage {
    Wgsl,
    Hlsl,
    Glsl,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ShaderStage {
    Vertex,
    Fragment,
    Compute,
    TesselationControl,
    TesselationEvaluation,
    Mesh,
    Task,
    Geometry,
    RayGeneration,
    ClosestHit,
    AnyHit,
    Callable,
    Miss,
    Intersect,
}

impl ToString for ShaderStage {
    fn to_string(&self) -> String {
        match self {
            ShaderStage::Vertex => "vertex".to_string(),
            ShaderStage::Fragment => "fragment".to_string(),
            ShaderStage::Compute => "compute".to_string(),
            ShaderStage::TesselationControl => "tesselationcontrol".to_string(),
            ShaderStage::TesselationEvaluation => "tesselationevaluation".to_string(),
            ShaderStage::Mesh => "mesh".to_string(),
            ShaderStage::Task => "task".to_string(),
            ShaderStage::Geometry => "geometry".to_string(),
            ShaderStage::RayGeneration => "raygeneration".to_string(),
            ShaderStage::ClosestHit => "closesthit".to_string(),
            ShaderStage::AnyHit => "anyhit".to_string(),
            ShaderStage::Callable => "callable".to_string(),
            ShaderStage::Miss => "miss".to_string(),
            ShaderStage::Intersect => "intersect".to_string(),
        }
    }
}

impl FromStr for ShadingLanguage {
    type Err = ();

    fn from_str(input: &str) -> Result<ShadingLanguage, Self::Err> {
        match input {
            "wgsl" => Ok(ShadingLanguage::Wgsl),
            "hlsl" => Ok(ShadingLanguage::Hlsl),
            "glsl" => Ok(ShadingLanguage::Glsl),
            _ => Err(()),
        }
    }
}
impl ToString for ShadingLanguage {
    fn to_string(&self) -> String {
        String::from(match &self {
            ShadingLanguage::Wgsl => "wgsl",
            ShadingLanguage::Hlsl => "hlsl",
            ShadingLanguage::Glsl => "glsl",
        })
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ShaderParameter {
    pub ty: String,
    pub label: String,
    pub description: String,
}

#[allow(non_snake_case)] // for JSON
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ShaderSignature {
    pub returnType: String,
    pub description: String,
    pub parameters: Vec<ShaderParameter>,
}

impl ShaderSignature {
    pub fn format(&self, label: &str) -> String {
        let signature = self
            .parameters
            .iter()
            .map(|p| format!("{} {}", p.ty, p.label))
            .collect::<Vec<String>>();
        format!("{} {}({})", self.returnType, label, signature.join(", "))
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ShaderPosition {
    pub file_path: PathBuf,
    pub line: u32,
    pub pos: u32,
}
impl Ord for ShaderPosition {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.file_path, &self.line, &self.pos).cmp(&(&other.file_path, &other.line, &other.pos))
    }
}

impl PartialOrd for ShaderPosition {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ShaderPosition {
    fn eq(&self, other: &Self) -> bool {
        (&self.file_path, &self.line, &self.pos) == (&other.file_path, &other.line, &other.pos)
    }
}

impl Eq for ShaderPosition {}

#[derive(Debug, Default, Clone)]
pub struct ShaderScope {
    pub start: ShaderPosition,
    pub end: ShaderPosition,
}

impl ShaderScope {
    pub fn is_in_range(&self, position: &ShaderPosition) -> bool {
        position.file_path == self.start.file_path
            && position.file_path == self.end.file_path
            && position.line >= self.start.line
            && position.line <= self.end.line
            && position.pos > self.start.pos
            && position.pos < self.end.pos
    }
}

#[allow(non_snake_case)] // for JSON
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ShaderSymbol {
    pub label: String,                      // Label for the item
    pub description: String,                // Description of the item
    pub version: String,                    // Minimum version required for the item.
    pub stages: Vec<ShaderStage>,           // Shader stages of the item
    pub link: Option<String>,               // Link to some external documentation
    pub signature: Option<ShaderSignature>, // Signature of function
    pub ty: Option<String>,                 // Type of variables
    pub position: Option<ShaderPosition>,   // Position in shader
    #[serde(skip)]
    pub scope_stack: Option<Vec<ShaderScope>>, // Stack of declaration
}

pub enum ShaderSymbolType {
    Types,
    Constants,
    Variables,
    Functions,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ShaderSymbolList {
    // Could use maps for faster search access (hover provider)
    pub types: Vec<ShaderSymbol>,
    pub constants: Vec<ShaderSymbol>,
    pub variables: Vec<ShaderSymbol>,
    pub functions: Vec<ShaderSymbol>,
}

impl ShaderSymbolList {
    pub fn parse_from_json(file_content: String) -> ShaderSymbolList {
        serde_json::from_str::<ShaderSymbolList>(&file_content)
            .expect("Failed to parse ShaderSymbolList")
    }
    pub fn find_symbols(&self, label: String) -> Vec<&ShaderSymbol> {
        let mut symbols = Vec::<&ShaderSymbol>::new();
        symbols.append(&mut self.functions.iter().filter(|e| e.label == label).collect());
        symbols.append(&mut self.constants.iter().filter(|e| e.label == label).collect());
        symbols.append(&mut self.types.iter().filter(|e| e.label == label).collect());
        symbols.append(&mut self.variables.iter().filter(|e| e.label == label).collect());
        symbols
    }
}

impl ShaderSymbol {
    pub fn new(
        name: String,
        description: String,
        version: String,
        stages: Vec<ShaderStage>,
    ) -> Self {
        Self {
            label: name,
            description: description,
            version: version,
            stages: stages,
            link: None,
            signature: None,
            ty: None,
            position: None,
            scope_stack: None,
        }
    }
    pub fn format(&self) -> String {
        match &self.signature {
            Some(signature) => signature.format(&self.label),
            None => match &self.ty {
                Some(ty) => format!("{} {}", ty, self.label),
                None => self.label.clone(),
            },
        }
    }
}

pub fn get_default_shader_completion(shading_language: ShadingLanguage) -> ShaderSymbolList {
    match shading_language {
        ShadingLanguage::Wgsl => ShaderSymbolList::parse_from_json(String::from(include_str!(
            "intrinsics/wgsl-intrinsics.json"
        ))),
        ShadingLanguage::Hlsl => ShaderSymbolList::parse_from_json(String::from(include_str!(
            "intrinsics/hlsl-intrinsics.json"
        ))),
        ShadingLanguage::Glsl => ShaderSymbolList::parse_from_json(String::from(include_str!(
            "intrinsics/glsl-intrinsics.json"
        ))),
    }
}
impl ShaderSymbolList {
    pub fn filter_shader_completion(&mut self, shader_stage: ShaderStage) {
        *self = ShaderSymbolList {
            types: self
                .types
                .drain(..)
                .filter(|value| value.stages.contains(&shader_stage) || value.stages.is_empty())
                .collect(),
            constants: self
                .constants
                .drain(..)
                .filter(|value| value.stages.contains(&shader_stage) || value.stages.is_empty())
                .collect(),
            variables: self
                .variables
                .drain(..)
                .filter(|value| value.stages.contains(&shader_stage) || value.stages.is_empty())
                .collect(),
            functions: self
                .functions
                .drain(..)
                .filter(|value| value.stages.contains(&shader_stage) || value.stages.is_empty())
                .collect(),
        }
    }
}

pub struct ValidationParams {
    pub includes: Vec<String>,
    pub defines: HashMap<String, String>,
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

pub fn get_shader_position(content: &str, pos: usize, file_path: &Path) -> ShaderPosition {
    let line = content[..pos].lines().count() - 1;
    let pos =
        content[pos..].as_ptr() as usize - content[..pos].lines().last().unwrap().as_ptr() as usize;
    ShaderPosition {
        line: line as u32,
        pos: pos as u32,
        file_path: PathBuf::from(file_path),
    }
}

impl ValidationParams {
    pub fn new(includes: Vec<String>, defines: HashMap<String, String>) -> Self {
        Self { includes, defines }
    }
}
