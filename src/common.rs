use std::{collections::HashMap, path::Path, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::shader_error::ShaderErrorList;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ShadingLanguage {
    Wgsl,
    Hlsl,
    Glsl,
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

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ShaderTree {
    pub types: Vec<String>,
    pub global_variables: Vec<String>,
    pub functions: Vec<String>,
}

pub struct ValidationParams {
    pub includes: Vec<String>,
    pub defines: HashMap<String, String>,
}

pub trait Validator {
    fn validate_shader(
        &mut self,
        path: &Path,
        cwd: &Path,
        params: ValidationParams,
    ) -> Result<(), ShaderErrorList>;
    fn get_shader_tree(
        &mut self,
        path: &Path,
        cwd: &Path,
        params: ValidationParams,
    ) -> Result<ShaderTree, ShaderErrorList>;
}

impl ValidationParams {
    pub fn new(includes: Vec<String>, defines: HashMap<String, String>) -> Self {
        Self { includes, defines }
    }
}
