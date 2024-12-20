#[cfg(not(target_os = "wasi"))]
pub mod dxc;
pub mod glslang;
pub mod naga;
pub mod validator;

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, path::Path};

    use super::validator::*;
    use super::*;

    fn include_callback(path: &Path) -> Option<String> {
        Some(std::fs::read_to_string(path).unwrap())
    }

    #[test]
    fn glsl_ok() {
        let mut validator = glslang::Glslang::glsl();
        let file_path = Path::new("./test/glsl/ok.frag.glsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        match validator.validate_shader(
            shader_content,
            file_path,
            ValidationParams::default(),
            &mut include_callback,
        ) {
            Ok(result) => {
                println!("Diagnostic should be empty: {:#?}", result.0);
                assert!(result.0.is_empty())
            }
            Err(err) => panic!("{}", err),
        }
    }

    #[test]
    fn glsl_include_config() {
        let mut validator = glslang::Glslang::glsl();
        let file_path = Path::new("./test/glsl/include-config.frag.glsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        match validator.validate_shader(
            shader_content,
            file_path,
            ValidationParams {
                includes: vec!["./test/glsl/inc0/".into()],
                ..Default::default()
            },
            &mut include_callback,
        ) {
            Ok(result) => {
                println!("Diagnostic should be empty: {:#?}", result.0);
                assert!(result.0.is_empty())
            }
            Err(err) => panic!("{}", err),
        };
    }

    #[test]
    fn glsl_include_level() {
        let mut validator = glslang::Glslang::glsl();
        let file_path = Path::new("./test/glsl/include-level.comp.glsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        match validator.validate_shader(
            shader_content,
            file_path,
            ValidationParams {
                includes: vec!["./test/glsl/inc0/".into()],
                ..Default::default()
            },
            &mut include_callback,
        ) {
            Ok(result) => {
                println!("Diagnostic should be empty: {:#?}", result.0);
                assert!(result.0.is_empty())
            }
            Err(err) => panic!("{}", err),
        };
    }

    #[test]
    fn glsl_no_stage() {
        let mut validator = glslang::Glslang::glsl();
        let file_path = Path::new("./test/glsl/nostage.glsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        match validator.validate_shader(
            shader_content,
            file_path,
            ValidationParams {
                includes: vec!["./test/glsl/inc0/".into()],
                ..Default::default()
            },
            &mut include_callback,
        ) {
            Ok(result) => {
                println!("Diagnostic should be empty: {:#?}", result.0);
                assert!(result.0.is_empty())
            }
            Err(err) => panic!("{}", err),
        };
    }

    #[test]
    fn glsl_macro() {
        let mut validator = glslang::Glslang::glsl();
        let file_path = Path::new("./test/glsl/macro.frag.glsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        match validator.validate_shader(
            shader_content,
            file_path,
            ValidationParams {
                defines: HashMap::from([("CUSTOM_MACRO".into(), "42".into())]),
                ..Default::default()
            },
            &mut include_callback,
        ) {
            Ok(result) => {
                println!("Diagnostic should be empty: {:#?}", result.0);
                assert!(result.0.is_empty())
            }
            Err(err) => panic!("{}", err),
        };
    }

    #[test]
    fn glsl_error_parsing() {
        let mut validator = glslang::Glslang::glsl();
        let file_path = Path::new("./test/glsl/error-parsing.frag.glsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        match validator.validate_shader(
            shader_content,
            file_path,
            ValidationParams::default(),
            &mut include_callback,
        ) {
            Ok(result) => {
                let diags = result.0.diagnostics;
                println!("Diagnostic should be empty: {:#?}", diags);
                assert!(diags[0].file_path.is_some());
                assert!(diags[0].file_path.as_ref().unwrap().exists());
                assert_eq!(diags[0].error, String::from(" '#include' : Could not process include directive for header name: ./level1.glsl\n"));
            }
            Err(err) => panic!("{}", err),
        };
    }

    #[test]
    fn hlsl_ok() {
        let mut validator = dxc::Dxc::new().unwrap();
        let file_path = Path::new("./test/hlsl/ok.hlsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        match validator.validate_shader(
            shader_content,
            file_path,
            ValidationParams::default(),
            &mut include_callback,
        ) {
            Ok(result) => {
                println!("Diagnostic should be empty: {:#?}", result.0);
                assert!(result.0.is_empty())
            }
            Err(err) => panic!("{}", err),
        };
    }

    #[test]
    fn hlsl_include_config() {
        let mut validator = dxc::Dxc::new().unwrap();
        let file_path = Path::new("./test/hlsl/include-config.hlsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        match validator.validate_shader(
            shader_content,
            file_path,
            ValidationParams {
                includes: vec!["./test/hlsl/inc0/".into()],
                ..Default::default()
            },
            &mut include_callback,
        ) {
            Ok(result) => {
                println!("Diagnostic should be empty: {:#?}", result.0);
                assert!(result.0.is_empty())
            }
            Err(err) => panic!("{}", err),
        };
    }

    #[test]
    fn hlsl_include_parent_folder() {
        let mut validator = dxc::Dxc::new().unwrap();
        let file_path = Path::new("./test/hlsl/folder/folder-file.hlsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        match validator.validate_shader(
            shader_content,
            file_path,
            ValidationParams {
                includes: vec!["./test/hlsl/".into()],
                ..Default::default()
            },
            &mut include_callback,
        ) {
            Ok(result) => {
                println!("Diagnostic should be empty: {:#?}", result.0);
                assert!(result.0.is_empty())
            }
            Err(err) => panic!("{}", err),
        };
    }

    #[test]
    fn hlsl_include_level() {
        let mut validator = dxc::Dxc::new().unwrap();
        let file_path = Path::new("./test/hlsl/include-level.hlsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        match validator.validate_shader(
            shader_content,
            file_path,
            ValidationParams {
                includes: vec!["./test/hlsl/inc0/".into()],
                ..Default::default()
            },
            &mut include_callback,
        ) {
            Ok(result) => {
                println!("Diagnostic should be empty: {:#?}", result.0);
                assert!(result.0.is_empty())
            }
            Err(err) => panic!("{}", err),
        };
    }

    #[test]
    fn hlsl_macro() {
        let mut validator = dxc::Dxc::new().unwrap();
        let file_path = Path::new("./test/hlsl/macro.hlsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        match validator.validate_shader(
            shader_content,
            file_path,
            ValidationParams {
                defines: HashMap::from([("CUSTOM_MACRO".into(), "42".into())]),
                ..Default::default()
            },
            &mut include_callback,
        ) {
            Ok(result) => {
                println!("Diagnostic should be empty: {:#?}", result.0);
                assert!(result.0.is_empty())
            }
            Err(err) => panic!("{}", err),
        };
    }

    #[test]
    fn hlsl_16bits_types_ok() {
        let mut validator = dxc::Dxc::new().unwrap();
        let file_path = Path::new("./test/hlsl/16bit-types.hlsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        match validator.validate_shader(
            shader_content,
            file_path,
            ValidationParams {
                hlsl_enable16bit_types: true,
                ..Default::default()
            },
            &mut include_callback,
        ) {
            Ok(result) => {
                println!("Diagnostic should be empty: {:#?}", result.0);
                assert!(result.0.is_empty())
            }
            Err(err) => panic!("{}", err),
        };
    }

    #[test]
    fn wgsl_ok() {
        let mut validator = naga::Naga::new();
        let file_path = Path::new("./test/wgsl/ok.wgsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        match validator.validate_shader(
            shader_content,
            file_path,
            ValidationParams::default(),
            &mut include_callback,
        ) {
            Ok(result) => {
                println!("Diagnostic should be empty: {:#?}", result.0);
                assert!(result.0.is_empty())
            }
            Err(err) => panic!("{}", err),
        };
    }
}
