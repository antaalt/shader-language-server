use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc};

use log::debug;
use lsp_types::Url;

use crate::{
    server::{clean_url, hover::lsp_range_to_shader_range, to_file_path},
    shaders::{
        shader::ShadingLanguage,
        symbols::symbols::{ShaderSymbolList, SymbolError, SymbolProvider},
        validator::{dxc::Dxc, glslang::Glslang, naga::Naga, validator::Validator},
    },
};

use super::server_config::ServerConfig;

pub type ServerFileCacheHandle = Rc<RefCell<ServerFileCache>>;

#[derive(Debug, Clone)]
pub struct ServerFileCache {
    pub shading_language: ShadingLanguage,
    pub content: String, // Store content on change as its not on disk.
    pub symbol_cache: ShaderSymbolList, // Store symbol to avoid computing them at every change.
    pub dependencies: HashMap<PathBuf, ServerFileCacheHandle>, // Store all dependencies of this file.
    pub is_main_file: bool, // Is the file a deps or is it open in editor.
}
pub struct ServerLanguageFileCache {
    pub files: HashMap<Url, ServerFileCacheHandle>,
}
pub struct ServerLanguageData {
    pub watched_files: ServerLanguageFileCache,
    pub validator: Box<dyn Validator>,
    pub symbol_provider: SymbolProvider,
    pub config: ServerConfig,
}

impl ServerLanguageFileCache {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }
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
impl ServerFileCache {
    pub fn update(
        &mut self,
        uri: &Url,
        symbol_provider: &mut SymbolProvider,
        config: &ServerConfig,
        range: Option<lsp_types::Range>,
        partial_content: Option<&String>,
    ) -> Result<(), SymbolError> {
        // TODO: split for clarity
        let now_start = std::time::Instant::now();
        let old_content = self.content.clone();
        let now_update_ast = std::time::Instant::now();
        // Update abstract syntax tree
        let file_path = to_file_path(&uri);
        let validation_params = config.into_validation_params();
        let new_content = if let (Some(range), Some(partial_content)) = (range, partial_content) {
            let shader_range = lsp_range_to_shader_range(&range, &file_path);
            let mut new_content = old_content.clone();
            new_content.replace_range(
                shader_range.start.to_byte_offset(&old_content)
                    ..shader_range.end.to_byte_offset(&old_content),
                &partial_content,
            );
            symbol_provider.update_ast(
                &file_path,
                &old_content,
                &new_content,
                &shader_range,
                &partial_content,
            )?;
            new_content
        } else if let Some(whole_content) = partial_content {
            symbol_provider.create_ast(&file_path, &whole_content)?;
            // if no range set, partial_content has whole content.
            whole_content.clone()
        } else {
            // Copy current content.
            self.content.clone()
        };
        debug!(
            "timing:update_watched_file_content:ast           {}ms",
            now_update_ast.elapsed().as_millis()
        );

        let now_get_symbol = std::time::Instant::now();
        // Cache symbols
        let symbol_list =
            symbol_provider.get_all_symbols(&new_content, &file_path, &validation_params)?;
        self.symbol_cache = if config.symbols {
            symbol_list
        } else {
            ShaderSymbolList::default()
        };
        self.content = new_content;
        debug!(
            "timing:update_watched_file_content:get_all_symb  {}ms",
            now_get_symbol.elapsed().as_millis()
        );
        debug!(
            "timing:update_watched_file_content:              {}ms",
            now_start.elapsed().as_millis()
        );
        Ok(())
    }
}
impl ServerLanguageFileCache {
    pub fn watch_file(
        &mut self,
        uri: &Url,
        lang: ShadingLanguage,
        text: &String,
        symbol_provider: &mut SymbolProvider,
        config: &ServerConfig,
        is_main_file: bool,
    ) -> Result<ServerFileCacheHandle, SymbolError> {
        let uri = clean_url(&uri);
        let file_path = to_file_path(&uri);

        // Check watched file already watched
        // TODO: instead we can pass file as optional param.
        let rc = match self.files.get(&uri) {
            Some(rc) => {
                if is_main_file {
                    debug!("File {} is opened in editor.", uri);
                    let mut rc_mut = RefCell::borrow_mut(rc);
                    rc_mut.is_main_file = true;
                    rc_mut.content = text.clone();
                    assert!(rc_mut.shading_language == lang);
                }
                Rc::clone(&rc)
            }
            None => {
                let rc = Rc::new(RefCell::new(ServerFileCache {
                    shading_language: lang,
                    content: text.clone(),
                    symbol_cache: if config.symbols {
                        let validation_params = config.into_validation_params();
                        symbol_provider.create_ast(&file_path, &text)?;
                        let symbol_list = symbol_provider.get_all_symbols(
                            &text,
                            &file_path,
                            &validation_params,
                        )?;
                        symbol_list
                    } else {
                        ShaderSymbolList::default()
                    },
                    dependencies: HashMap::new(), // Will be filled by validator.
                    is_main_file,
                }));
                let none = self.files.insert(uri.clone(), Rc::clone(&rc));
                assert!(none.is_none());
                rc
            }
        };

        debug!(
            "Starting watching {:#?} file at {} (is deps: {})",
            lang,
            file_path.display(),
            !is_main_file
        );
        Ok(rc)
    }
    pub fn get_watched_file(&self, uri: &Url) -> Option<ServerFileCacheHandle> {
        assert!(*uri == clean_url(&uri));
        match self.files.get(uri) {
            Some(cached_file) => Some(Rc::clone(&cached_file)),
            None => None,
        }
    }
    pub fn remove_watched_file(
        &mut self,
        uri: &Url,
        symbol_provider: &mut SymbolProvider,
        _config: &ServerConfig,
        is_main_file: bool,
    ) -> Result<bool, SymbolError> {
        fn list_all_dependencies_count(
            file_cache: &ServerFileCacheHandle,
        ) -> HashMap<PathBuf, usize> {
            let list = HashMap::new();
            for dependency in &RefCell::borrow(file_cache).dependencies {
                let mut list = HashMap::new();
                let deps = list_all_dependencies_count(&dependency.1);
                for dep in deps {
                    match list.get_mut(&dep.0) {
                        Some(count) => {
                            *count = *count + 1;
                        }
                        None => {
                            list.insert(dep.0, 1);
                        }
                    }
                }
            }
            list
        }
        // Look if its used by some deps before removing.
        match self.files.get(&uri) {
            Some(rc) => {
                let _is_main_file = if is_main_file {
                    let mut rc = RefCell::borrow_mut(rc);
                    rc.is_main_file = false;
                    false
                } else {
                    RefCell::borrow(rc).is_main_file
                };
                let file_path = to_file_path(&uri);
                let lang = RefCell::borrow(rc).shading_language;

                debug!(
                    "Removing watched file {} with ref count {}",
                    file_path.display(),
                    Rc::strong_count(rc)
                );

                // Collect all dangling deps
                let dependencies_count = list_all_dependencies_count(rc);

                // Check if file is ref in its own deps (might happen).
                let is_last_ref = match dependencies_count.get(&file_path) {
                    Some(count) => *count + 1 == Rc::strong_count(rc),
                    None => true,
                };
                if is_last_ref {
                    self.files.remove(&uri);

                    // Remove every dangling deps
                    for (dependency_path, dependency_count) in dependencies_count {
                        let url = Url::from_file_path(&dependency_path).unwrap();
                        match self.files.get(&url) {
                            Some(dependency_file) => {
                                let ref_count = Rc::strong_count(dependency_file);
                                let is_open_in_editor =
                                    RefCell::borrow(&dependency_file).is_main_file;
                                let is_dangling =
                                    ref_count == dependency_count + 1 && !is_open_in_editor;
                                if is_dangling {
                                    match symbol_provider.remove_ast(&dependency_path) {
                                        Ok(_) => {}
                                        Err(err) => {
                                            return Err(SymbolError::InternalErr(format!(
                                                "Error removing AST for file {}: {:#?}",
                                                dependency_path.display(),
                                                err
                                            )))
                                        }
                                    }
                                    self.files.remove(&url).unwrap();
                                    debug!(
                                        "Removed dangling {:#?} file at {}",
                                        lang,
                                        dependency_path.display()
                                    );
                                }
                            }
                            None => {
                                panic!("Could not find watched file {}", dependency_path.display())
                            }
                        }
                    }
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            None => Err(SymbolError::InternalErr(format!(
                "Trying to remove file {} that is not watched",
                uri.path()
            ))),
        }
    }
}
