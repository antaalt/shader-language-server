use std::{collections::HashMap, path::Path, str::FromStr};

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
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ShaderSymbolList {
    // Could use maps for faster search access (hover provider)
    pub types: Vec<ShaderSymbol>,
    pub constants: Vec<ShaderSymbol>,
    pub global_variables: Vec<ShaderSymbol>,
    pub functions: Vec<ShaderSymbol>,
}

impl ShaderSymbolList {
    pub fn parse_from_json(file_content: String) -> ShaderSymbolList {
        serde_json::from_str::<ShaderSymbolList>(&file_content)
            .expect("Failed to parse ShaderSymbolList")
    }
    pub fn find_symbol(&self, label: String) -> Option<&ShaderSymbol> {
        match self.functions.iter().find(|e| e.label == label) {
            Some(symbol) => return Some(symbol),
            None => {}
        }
        match self.constants.iter().find(|e| e.label == label) {
            Some(symbol) => return Some(symbol),
            None => {}
        }
        match self.types.iter().find(|e| e.label == label) {
            Some(symbol) => return Some(symbol),
            None => {}
        }
        match self.global_variables.iter().find(|e| e.label == label) {
            Some(symbol) => return Some(symbol),
            None => {}
        }
        None
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
            global_variables: self
                .global_variables
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

impl ValidationParams {
    pub fn new(includes: Vec<String>, defines: HashMap<String, String>) -> Self {
        Self { includes, defines }
    }
}
