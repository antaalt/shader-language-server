use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone)]
pub struct Dependencies {
    dependencies: Vec<PathBuf>,
}

pub struct IncludeHandler {
    includes: Vec<String>,
    directory_stack: Vec<PathBuf>, // Could be replace by deps.
    dependencies: Dependencies,
}

impl Dependencies {
    pub fn new() -> Self {
        Self {
            dependencies: Vec::new(),
        }
    }
    pub fn add_dependency(&mut self, relative_path: PathBuf) {
        self.dependencies.push(
            std::fs::canonicalize(&relative_path)
                .expect("Failed to convert dependency path to absolute"),
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
    fn read(&self, path: &Path) -> Option<String> {
        match std::fs::read_to_string(path) {
            Ok(content) => Some(content),
            Err(_) => None,
        }
    }
    pub fn search_in_includes(&mut self, relative_path: &Path) -> Option<(String, PathBuf)> {
        if relative_path.exists() {
            self.read(&relative_path)
                .map(|e| (e, PathBuf::from(relative_path)))
        } else {
            // Check directory stack.
            for directory_stack in &self.directory_stack {
                let path = Path::new(directory_stack).join(&relative_path);
                if let Some(content) = self.read(&path) {
                    if let Some(parent) = path.parent() {
                        self.directory_stack.push(PathBuf::from(parent));
                    }
                    self.dependencies.add_dependency(path.clone());
                    return Some((content, path));
                }
            }
            // Check include paths
            for include_path in &self.includes {
                let path = Path::new(include_path).join(&relative_path);
                if let Some(content) = self.read(&path) {
                    if let Some(parent) = path.parent() {
                        self.directory_stack.push(PathBuf::from(parent));
                    }
                    self.dependencies.add_dependency(path.clone());
                    return Some((content, path));
                }
            }
            return None;
        }
    }
    pub fn get_dependencies(&self) -> &Dependencies {
        return &self.dependencies;
    }
}
