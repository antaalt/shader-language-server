mod glsl;
pub mod symbols;

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::shaders::shader::ShadingLanguage;

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
        // Ensure parsing of intrinsics is OK
        let file_path = Path::new("./test/glsl/error-parsing.frag.glsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        let symbol_provider = SymbolProvider::glsl();
        let symbols = symbol_provider.capture(&shader_content, file_path, vec![]);
        assert!(!symbols.functions.is_empty());
    }
    #[test]
    fn symbols_hlsl_ok() {
        // Ensure parsing of intrinsics is OK
        let file_path = Path::new("./test/glsl/error-parsing.frag.glsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        let symbol_provider = SymbolProvider::hlsl();
        let symbols = symbol_provider.capture(&shader_content, file_path, vec![]);
        assert!(symbols.functions.is_empty());
    }
    #[test]
    fn symbols_wgsl_ok() {
        // Ensure parsing of intrinsics is OK
        let file_path = Path::new("./test/glsl/error-parsing.frag.glsl");
        let shader_content = std::fs::read_to_string(file_path).unwrap();
        let symbol_provider = SymbolProvider::wgsl();
        let symbols = symbol_provider.capture(&shader_content, file_path, vec![]);
        assert!(symbols.functions.is_empty());
    }
}
