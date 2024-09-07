mod glsl;
mod hlsl;
pub mod symbols;

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        path::{Path, PathBuf},
    };

    use crate::shaders::{
        shader::ShadingLanguage, symbols::symbols::ShaderPosition,
        validator::validator::ValidationParams,
    };

    use super::symbols::{get_default_shader_completion, SymbolProvider};

    #[test]
    fn intrinsics_glsl_ok() {
        // Ensure parsing of intrinsics is OK
        let _ = get_default_shader_completion(ShadingLanguage::Glsl);
    }
    #[test]
    fn intrinsics_hlsl_ok() {
        // Ensure parsing of intrinsics is OK
        let _ = get_default_shader_completion(ShadingLanguage::Hlsl);
    }
    #[test]
    fn intrinsics_wgsl_ok() {
        // Ensure parsing of intrinsics is OK
        let _ = get_default_shader_completion(ShadingLanguage::Wgsl);
    }
    #[test]
    fn symbols_glsl_ok() {
        // Ensure parsing of symbols is OK
        let file_path = Path::new("./test/glsl/include-level.comp.glsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        let symbol_provider = SymbolProvider::glsl();
        let symbols = symbol_provider.get_all_symbols(
            &shader_content,
            file_path,
            &ValidationParams::new(Vec::new(), HashMap::new()),
        );
        assert!(!symbols.functions.is_empty());
    }
    #[test]
    fn symbols_hlsl_ok() {
        // Ensure parsing of symbols is OK
        let file_path = Path::new("./test/hlsl/include-level.hlsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        let symbol_provider = SymbolProvider::hlsl();
        let symbols = symbol_provider.get_all_symbols(
            &shader_content,
            file_path,
            &ValidationParams::new(Vec::new(), HashMap::new()),
        );
        assert!(!symbols.functions.is_empty());
    }
    #[test]
    fn symbols_wgsl_ok() {
        // Ensure parsing of symbols is OK
        let file_path = Path::new("./test/wgsl/ok.wgsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        let symbol_provider = SymbolProvider::wgsl();
        let symbols = symbol_provider.get_all_symbols(
            &shader_content,
            file_path,
            &ValidationParams::new(Vec::new(), HashMap::new()),
        );
        assert!(symbols.functions.is_empty());
    }
    #[test]
    fn symbol_scope_glsl_ok() {
        let file_path = Path::new("./test/glsl/scopes.frag.glsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        let symbol_provider = SymbolProvider::glsl();
        let symbols = symbol_provider.get_all_symbols_in_scope(
            &shader_content,
            file_path,
            &ValidationParams::new(Vec::new(), HashMap::new()),
            Some(ShaderPosition {
                file_path: PathBuf::from(file_path),
                line: 16,
                pos: 0,
            }),
        );
        let variables_visibles: Vec<String> = vec![
            "scopeRoot".into(),
            "scope1".into(),
            "scopeGlobal".into(),
            "level1".into(),
        ];
        let variables_not_visibles: Vec<String> = vec!["scope2".into(), "testData".into()];
        for variable_visible in variables_visibles {
            assert!(
                symbols
                    .variables
                    .iter()
                    .any(|e| e.label == variable_visible),
                "Failed to find variable {}",
                variable_visible
            );
        }
        for variable_not_visible in variables_not_visibles {
            assert!(
                !symbols
                    .variables
                    .iter()
                    .any(|e| e.label == variable_not_visible),
                "Found variable {}",
                variable_not_visible
            );
        }
    }
}
