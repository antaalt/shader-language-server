use core::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum ShaderErrorSeverity {
    Error,
    Warning,
    Information,
    Hint,
}
impl fmt::Display for ShaderErrorSeverity {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ShaderErrorSeverity::Error => write!(f, "error"),
            ShaderErrorSeverity::Warning => write!(f, "warning"),
            ShaderErrorSeverity::Information => write!(f, "info"),
            ShaderErrorSeverity::Hint => write!(f, "hint"),
        }
    }
}

impl From<String> for ShaderErrorSeverity {
    fn from(value: String) -> Self {
        match value.as_str() {
            "error" => ShaderErrorSeverity::Error,
            "warning" => ShaderErrorSeverity::Warning,
            "info" => ShaderErrorSeverity::Information,
            "hint" => ShaderErrorSeverity::Hint,
            _ => ShaderErrorSeverity::Error,
        }
    }
}

impl ShaderErrorSeverity {
    pub fn is_required(&self, required_severity: ShaderErrorSeverity) -> bool {
        self.get_enum_index() <= required_severity.get_enum_index()
    }
    fn get_enum_index(&self) -> u32 {
        match self {
            ShaderErrorSeverity::Error => 0,
            ShaderErrorSeverity::Warning => 1,
            ShaderErrorSeverity::Information => 2,
            ShaderErrorSeverity::Hint => 3,
        }
    }
}

#[derive(Debug)]
pub struct ShaderDiagnostic {
    pub file_path: Option<PathBuf>,
    pub severity: ShaderErrorSeverity,
    pub error: String,
    pub line: u32,
    pub pos: u32,
}
#[derive(Debug)]
pub struct ShaderDiagnosticList {
    pub diagnostics: Vec<ShaderDiagnostic>,
}

#[derive(Debug)]
pub enum ValidatorError {
    IoErr(std::io::Error),
    InternalErr(String),
}

impl From<regex::Error> for ValidatorError {
    fn from(error: regex::Error) -> Self {
        match error {
            regex::Error::CompiledTooBig(err) => {
                ValidatorError::internal(format!("Regex compile too big: {}", err))
            }
            regex::Error::Syntax(err) => {
                ValidatorError::internal(format!("Regex syntax invalid: {}", err))
            }
            err => ValidatorError::internal(format!("Regex error: {:#?}", err)),
        }
    }
}

impl fmt::Display for ValidatorError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ValidatorError::IoErr(err) => write!(f, "IoError: {}", err),
            ValidatorError::InternalErr(err) => write!(f, "Error: {}", err),
        }
    }
}

impl From<std::io::Error> for ValidatorError {
    fn from(err: std::io::Error) -> Self {
        ValidatorError::IoErr(err)
    }
}
impl From<ShaderDiagnostic> for ShaderDiagnosticList {
    fn from(err: ShaderDiagnostic) -> Self {
        Self {
            diagnostics: vec![err],
        }
    }
}
impl ShaderDiagnosticList {
    pub fn empty() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }
    pub fn push(&mut self, error: ShaderDiagnostic) {
        self.diagnostics.push(error);
    }
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }
}
impl ValidatorError {
    pub fn internal(error: String) -> Self {
        ValidatorError::InternalErr(error)
    }
}
pub enum ShaderError {
    Validator(ValidatorError),
    DiagnosticList(ShaderDiagnosticList),
}

impl From<ShaderError> for ValidatorError {
    fn from(value: ShaderError) -> Self {
        match value {
            ShaderError::Validator(validator) => validator,
            ShaderError::DiagnosticList(diag) => ValidatorError::internal(format!("Trying to convert ValidatorError to ShaderError, but we received diagnostic: {:#?}", diag)),
        }
    }
}
