use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Debug, Default, Clone)]
pub struct Dependencies {
    dependencies: HashSet<PathBuf>,
}

pub struct IncludeHandler {
    includes: Vec<String>,
    directory_stack: Vec<PathBuf>, // Could be replace by deps.
    dependencies: Dependencies,    // TODO: Remove
}
// std::fs::canonicalize not supported on wasi target... Emulate it.
// On Windows, std::fs::canonicalize return a /? prefix that break hashmap.
// https://stackoverflow.com/questions/50322817/how-do-i-remove-the-prefix-from-a-canonical-windows-path
// Instead use a custom canonicalize.
pub fn canonicalize(p: &Path) -> std::io::Result<PathBuf> {
    // https://github.com/antmicro/wasi_ext_lib/blob/main/canonicalize.patch
    fn __canonicalize(path: &Path, buf: &mut PathBuf) {
        if path.is_absolute() {
            buf.clear();
        }
        for part in path {
            if part == ".." {
                buf.pop();
            } else if part != "." {
                buf.push(part);
                if let Ok(linkpath) = buf.read_link() {
                    buf.pop();
                    __canonicalize(&linkpath, buf);
                }
            }
        }
    }
    let mut path = if p.is_absolute() {
        PathBuf::new()
    } else {
        PathBuf::from(std::env::current_dir()?)
    };
    __canonicalize(p, &mut path);
    Ok(path)
}

impl Dependencies {
    pub fn new() -> Self {
        Self {
            dependencies: HashSet::new(),
        }
    }
    pub fn add_dependency(&mut self, relative_path: PathBuf) {
        self.dependencies.insert(
            canonicalize(&relative_path).expect("Failed to convert dependency path to absolute"),
        );
    }
    pub fn visit_dependencies<F: FnMut(&Path)>(&self, callback: &mut F) {
        for dependency in &self.dependencies {
            callback(&dependency);
        }
    }
}

impl IncludeHandler {
    pub fn new(file: &Path, includes: Vec<String>) -> Self {
        // Add local path to include path
        let mut includes_mut = includes;
        let cwd = file.parent().unwrap();
        let str = String::from(cwd.to_string_lossy());
        // TODO: push cwd in first. Or move it elsewhere
        includes_mut.push(str);
        Self {
            includes: includes_mut,
            directory_stack: Vec::new(),
            dependencies: Dependencies::new(),
        }
    }
    pub fn search_in_includes(
        &mut self,
        relative_path: &Path,
        include_callback: &mut dyn FnMut(&Path) -> Option<String>,
    ) -> Option<(String, PathBuf)> {
        match self.search_path_in_includes(relative_path) {
            Some(absolute_path) => include_callback(&absolute_path).map(|e| (e, absolute_path)),
            None => None,
        }
    }
    pub fn search_path_in_includes(&mut self, relative_path: &Path) -> Option<PathBuf> {
        self.search_path_in_includes_relative(relative_path)
            .map(|e| canonicalize(&e).expect("Failed to convert relative path to absolute"))
    }
    pub fn search_path_in_includes_relative(&mut self, relative_path: &Path) -> Option<PathBuf> {
        if relative_path.exists() {
            Some(PathBuf::from(relative_path))
        } else {
            // Check directory stack.
            for directory_stack in &self.directory_stack {
                let path = Path::new(directory_stack).join(&relative_path);
                if path.exists() {
                    if let Some(parent) = path.parent() {
                        // TODO: should filter paths
                        self.directory_stack.push(PathBuf::from(parent));
                    }
                    self.dependencies.add_dependency(path.clone());
                    return Some(path);
                }
            }
            // Check include paths
            for include_path in &self.includes {
                let path = Path::new(include_path).join(&relative_path);
                if path.exists() {
                    if let Some(parent) = path.parent() {
                        // TODO: should filter paths
                        self.directory_stack.push(PathBuf::from(parent));
                    }
                    self.dependencies.add_dependency(path.clone());
                    return Some(path);
                }
            }
            return None;
        }
    }
    pub fn get_dependencies(&self) -> &Dependencies {
        return &self.dependencies;
    }
}
