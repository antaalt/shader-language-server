use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use serde::{Deserialize, Serialize};

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