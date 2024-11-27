use std::cell::RefCell;

use crate::shaders::{
    symbols::symbols::{ShaderSymbolList, SymbolProvider},
    validator::{glslang::Glslang, naga::Naga, validator::Validator},
};

#[cfg(not(target_os = "wasi"))]
use crate::shaders::validator::dxc::Dxc;

use super::{
    server_config::ServerConfig,
    server_file_cache::{ServerFileCacheHandle, ServerLanguageFileCache},
};

pub struct ServerLanguageData {
    pub watched_files: ServerLanguageFileCache,
    pub validator: Box<dyn Validator>,
    pub symbol_provider: SymbolProvider,
    pub config: ServerConfig,
}

impl ServerLanguageData {
    pub fn glsl() -> Self {
        Self {
            watched_files: ServerLanguageFileCache::new(),
            validator: Box::new(Glslang::glsl()),
            symbol_provider: SymbolProvider::glsl(),
            config: ServerConfig::default(),
        }
    }
    pub fn hlsl() -> Self {
        Self {
            watched_files: ServerLanguageFileCache::new(),
            #[cfg(target_os = "wasi")]
            validator: Box::new(Glslang::hlsl()),
            #[cfg(not(target_os = "wasi"))]
            validator: Box::new(Dxc::new().unwrap()),
            symbol_provider: SymbolProvider::hlsl(),
            config: ServerConfig::default(),
        }
    }
    pub fn wgsl() -> Self {
        Self {
            watched_files: ServerLanguageFileCache::new(),
            validator: Box::new(Naga::new()),
            symbol_provider: SymbolProvider::wgsl(),
            config: ServerConfig::default(),
        }
    }
    pub fn get_all_symbols(&self, cached_file: ServerFileCacheHandle) -> ShaderSymbolList {
        let cached_file = RefCell::borrow(&cached_file);
        // Add current symbols
        let mut symbol_cache = cached_file.symbol_cache.clone();
        // Add intrinsics symbols
        symbol_cache.append(self.symbol_provider.get_intrinsics_symbol().clone());
        // Add deps symbols
        for (_, deps_cached_file) in &cached_file.dependencies {
            let deps_cached_file = RefCell::borrow(&deps_cached_file);
            symbol_cache.append(deps_cached_file.symbol_cache.clone());
        }
        symbol_cache
    }
}
