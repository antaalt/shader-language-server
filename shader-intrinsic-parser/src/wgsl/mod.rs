use shader_sense::symbols::symbols::ShaderSymbolList;

use crate::common::IntrinsicParser;

pub struct WgslIntrinsicParser {}

impl IntrinsicParser for WgslIntrinsicParser {
    fn cache(&self, _cache_path: &str) {}
    fn parse(&self, _cache_path: &str) -> ShaderSymbolList {
        ShaderSymbolList::default() // TODO:
    }
}
