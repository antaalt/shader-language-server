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
impl ToString for ShadingLanguage {
    fn to_string(&self) -> String {
        String::from(match &self {
            ShadingLanguage::Wgsl =>"wgsl",
            ShadingLanguage::Hlsl =>"hlsl",
            ShadingLanguage::Glsl =>"glsl",
        })
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
        shader_content: String,
        file_path: &Path,
        params: ValidationParams,
    ) -> Result<(), ShaderErrorList>;
    fn get_shader_tree(
        &mut self,
        path: &Path,
        params: ValidationParams,
    ) -> Result<ShaderTree, ShaderErrorList>;

    fn get_file_name(&self, path: &Path) -> String {
        String::from(path.file_name().unwrap_or_default().to_string_lossy())
    }

    fn get_cwd<'a>(&self, path: &'a Path) -> &'a Path {
        path.parent().expect("Failed to retrieve cwd")
    }
}

impl ValidationParams {
    pub fn new(includes: Vec<String>, defines: HashMap<String, String>) -> Self {
        Self { includes, defines }
    }
}
