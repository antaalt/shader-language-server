use std::path::Path;

pub struct IncludeHandler {
    includes: Vec<String>,
}
impl IncludeHandler {
    pub fn new(cwd: &Path, includes: Vec<String>) -> Self {
        // Add local path to include path
        let mut includes_mut = includes;
        let str = String::from(cwd.to_string_lossy());
        // TODO: push cwd in first. Or move it elsewhere
        includes_mut.push(str);
        Self {
            includes: includes_mut,
        }
    }
    fn read(&self, path: &Path) -> Option<String> {
        use std::io::Read;
        match std::fs::File::open(path) {
            Ok(mut f) => {
                let mut content = String::new();
                f.read_to_string(&mut content).ok()?;
                Some(content)
            }
            Err(_) => None,
        }
    }
    pub fn search_in_includes(&self, filename: &Path) -> Option<String> {
        if filename.exists() {
            return self.read(&filename);
        } else {
            for include in &self.includes {
                let path = Path::new(include).join(&filename);
                let content = self.read(&path);
                if content.is_some() {
                    return content;
                }
            }
            return None;
        }
    }
}
