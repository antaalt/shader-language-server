use shader_sense::symbols::symbols::{ShaderSymbol, ShaderSymbolData, ShaderSymbolList};

use super::GlslIntrinsicParser;

impl GlslIntrinsicParser {
    pub fn add_types(&self, symbols: &mut ShaderSymbolList) {
        pub fn new_glsl_type(label: &str, description: &str, version: &str) -> ShaderSymbol {
            ShaderSymbol {
                label: label.into(),
                description: description.into(),
                version: version.to_string(),
                stages: vec![],
                link: None,
                data: ShaderSymbolData::Types { ty: label.into() },
                range: None,
                scope_stack: None,
            }
        }
        // Manually push types as they are not in documentation
        symbols.types.push(new_glsl_type(
            "bool",
            "conditional type, values may be either true or false",
            "110",
        ));
        symbols.types.push(new_glsl_type(
            "int",
            " a signed, two's complement, 32-bit integer",
            "110",
        ));
        symbols
            .types
            .push(new_glsl_type("uint", " an unsigned 32-bit integer", "110"));
        symbols.types.push(new_glsl_type(
            "float",
            "an IEEE-754 single-precision floating point number",
            "110",
        ));
        symbols.types.push(new_glsl_type(
            "double",
            "an IEEE-754 double-precision floating-point number",
            "110",
        ));
        for component in 2..=4 {
            // Vectors
            symbols.types.push(new_glsl_type(
                format!("bvec{}", component).as_str(),
                format!("Vector with {} components of booleans", component).as_str(),
                "110",
            ));
            symbols.types.push(new_glsl_type(
                format!("ivec{}", component).as_str(),
                format!("Vector with {} components of signed integers", component).as_str(),
                "110",
            ));
            symbols.types.push(new_glsl_type(
                format!("uvec{}", component).as_str(),
                format!("Vector with {} components of unsigned integers", component).as_str(),
                "110",
            ));
            symbols.types.push(new_glsl_type(
                format!("vec{}", component).as_str(),
                format!(
                    "Vector with {} components of single-precision floating-point numbers",
                    component
                )
                .as_str(),
                "110",
            ));
            symbols.types.push(new_glsl_type(
                format!("dvec{}", component).as_str(),
                format!(
                    "Vector with {} components of double-precision floating-point numbers",
                    component
                )
                .as_str(),
                "110",
            ));
            // Matrices
            symbols.types.push(new_glsl_type(
                format!("mat{}", component).as_str(),
                format!(
                    "Matrice with {} columns & rows of single-precision floating-point numbers",
                    component
                )
                .as_str(),
                "110",
            ));
            symbols.types.push(new_glsl_type(
                format!("dmat{}", component).as_str(),
                format!(
                    "Matrice with {} columns & rows of double-precision floating-point numbers",
                    component
                )
                .as_str(),
                "110",
            ));
            for component_row in 2..=4 {
                symbols.types.push(new_glsl_type(format!("mat{}x{}", component, component_row).as_str(), format!("Matrice with {} columns and {} rows of single-precision floating-point numbers", component, component_row).as_str(), "110"));
                symbols.types.push(new_glsl_type(format!("dmat{}x{}", component, component_row).as_str(), format!("Matrice with {} columns and {} rows of double-precision floating-point numbers", component, component_row).as_str(), "110"));
            }
        }
        // Samplers
        let sampler_types = [
            "1D",
            "2D",
            "3D",
            "Cube",
            "2DRect",
            "1DArray",
            "2DArray",
            "CubeArray",
            "Buffer",
            "2DMS",
            "2DMSArray",
        ];
        for sampler_type in sampler_types {
            symbols.types.push(new_glsl_type(
                format!("sampler{}", sampler_type).as_str(),
                format!("Floating-point sampler for Texture{}", sampler_type).as_str(),
                "110",
            ));
            symbols.types.push(new_glsl_type(
                format!("isampler{}", sampler_type).as_str(),
                format!("Signed integer sampler for Texture{}", sampler_type).as_str(),
                "110",
            ));
            symbols.types.push(new_glsl_type(
                format!("usampler{}", sampler_type).as_str(),
                format!("Unsigned integer sampler for Texture{}", sampler_type).as_str(),
                "110",
            ));
        }
        // Shadow Samplers
        let shadow_sampler_types = [
            "1D",
            "2D",
            "Cube",
            "2DRect",
            "1DArray",
            "2DArray",
            "CubeArray",
        ];
        for shadow_sampler_type in shadow_sampler_types {
            symbols.types.push(new_glsl_type(
                format!("sampler{}Shadow", shadow_sampler_type).as_str(),
                format!("Shadow sampler for Texture{}", shadow_sampler_type).as_str(),
                "110",
            ));
        }
        // Atomic counters
        symbols.types.push(new_glsl_type("atomic_uint", "An Atomic Counter is a GLSL variable type whose storage comes from a Buffer Object. Atomic counters, as the name suggests, can have atomic memory operations performed on them. They can be thought of as a very limited form of buffer image variable.", "460"));
    }
}
