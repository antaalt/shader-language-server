mod glsl;
pub mod symbols;


#[cfg(test)]
mod tests {
    use crate::shaders::shader::ShadingLanguage;

    use super::symbols::get_default_shader_completion;

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
}