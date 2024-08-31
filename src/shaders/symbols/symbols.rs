use std::{
    cmp::Ordering,
    path::{Path, PathBuf},
};

use log::error;
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};

use crate::shaders::{
    include::IncludeHandler,
    shader::{ShaderStage, ShadingLanguage},
    validator::validator::ValidationParams,
};

use super::glsl::{
    GlslFunctionParser, GlslMacroParser, GlslStageFilter, GlslStructParser, GlslVariableParser,
    GlslVersionFilter,
};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ShaderParameter {
    pub ty: String,
    pub label: String,
    pub description: String,
}

#[allow(non_snake_case)] // for JSON
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ShaderSignature {
    pub returnType: String,
    pub description: String,
    pub parameters: Vec<ShaderParameter>,
}

impl ShaderSignature {
    pub fn format(&self, label: &str) -> String {
        let signature = self
            .parameters
            .iter()
            .map(|p| format!("{} {}", p.ty, p.label))
            .collect::<Vec<String>>();
        format!("{} {}({})", self.returnType, label, signature.join(", "))
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ShaderPosition {
    pub file_path: PathBuf,
    pub line: u32,
    pub pos: u32,
}
impl Ord for ShaderPosition {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.file_path, &self.line, &self.pos).cmp(&(&other.file_path, &other.line, &other.pos))
    }
}

impl PartialOrd for ShaderPosition {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ShaderPosition {
    fn eq(&self, other: &Self) -> bool {
        (&self.file_path, &self.line, &self.pos) == (&other.file_path, &other.line, &other.pos)
    }
}

impl Eq for ShaderPosition {}

impl ShaderPosition {
    pub fn from_pos(content: &str, pos: usize, file_path: &Path) -> ShaderPosition {
        let line = content[..pos].lines().count() - 1;
        let pos = content[pos..].as_ptr() as usize
            - content[..pos].lines().last().unwrap().as_ptr() as usize;
        ShaderPosition {
            line: line as u32,
            pos: pos as u32,
            file_path: PathBuf::from(file_path),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ShaderScope {
    pub start: ShaderPosition,
    pub end: ShaderPosition,
}

impl ShaderScope {
    pub fn is_in_range(&self, position: &ShaderPosition) -> bool {
        assert!(self.start.file_path == self.end.file_path, "Position start & end should have same value.");
        // Check same file
        if position.file_path == self.start.file_path {
            // Check line & position bounds.
            if position.line > self.start.line && position.line < self.end.line {
                true
            } else if position.line == self.start.line && position.line == self.end.line {
                position.pos > self.start.pos && position.pos < self.end.pos
            } else if position.line == self.start.line && position.line < self.end.line {
                position.pos > self.start.pos
            } else if position.line == self.end.line && position.line > self.start.line {
                position.pos < self.end.pos
            } else {
                false
            }
        } else {
            false
        }
    }
}

#[allow(non_snake_case)] // for JSON
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ShaderSymbol {
    pub label: String,                      // Label for the item
    pub description: String,                // Description of the item
    pub version: String,                    // Minimum version required for the item.
    pub stages: Vec<ShaderStage>,           // Shader stages of the item
    pub link: Option<String>,               // Link to some external documentation
    pub signature: Option<ShaderSignature>, // Signature of function
    pub ty: Option<String>,                 // Type of variables
    pub position: Option<ShaderPosition>,   // Position in shader
    #[serde(skip)]
    pub scope_stack: Option<Vec<ShaderScope>>, // Stack of declaration
}
pub enum ShaderSymbolType {
    Types,
    Constants,
    Variables,
    Functions,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ShaderSymbolList {
    // Could use maps for faster search access (hover provider)
    pub types: Vec<ShaderSymbol>,
    pub constants: Vec<ShaderSymbol>,
    pub variables: Vec<ShaderSymbol>,
    pub functions: Vec<ShaderSymbol>,
}

impl ShaderSymbolList {
    pub fn parse_from_json(file_content: String) -> ShaderSymbolList {
        serde_json::from_str::<ShaderSymbolList>(&file_content)
            .expect("Failed to parse ShaderSymbolList")
    }
    pub fn find_symbols(&self, label: String) -> Vec<&ShaderSymbol> {
        let mut symbols = Vec::<&ShaderSymbol>::new();
        symbols.append(&mut self.functions.iter().filter(|e| e.label == label).collect());
        symbols.append(&mut self.constants.iter().filter(|e| e.label == label).collect());
        symbols.append(&mut self.types.iter().filter(|e| e.label == label).collect());
        symbols.append(&mut self.variables.iter().filter(|e| e.label == label).collect());
        symbols
    }
}

impl ShaderSymbol {
    pub fn new(
        name: String,
        description: String,
        version: String,
        stages: Vec<ShaderStage>,
    ) -> Self {
        Self {
            label: name,
            description: description,
            version: version,
            stages: stages,
            link: None,
            signature: None,
            ty: None,
            position: None,
            scope_stack: None,
        }
    }
    pub fn format(&self) -> String {
        match &self.signature {
            Some(signature) => signature.format(&self.label),
            None => match &self.ty {
                Some(ty) => format!("{} {}", ty, self.label),
                None => self.label.clone(),
            },
        }
    }
}

pub fn get_default_shader_completion(shading_language: ShadingLanguage) -> ShaderSymbolList {
    match shading_language {
        ShadingLanguage::Wgsl => ShaderSymbolList::parse_from_json(String::from(include_str!(
            "intrinsics/wgsl-intrinsics.json"
        ))),
        ShadingLanguage::Hlsl => ShaderSymbolList::parse_from_json(String::from(include_str!(
            "intrinsics/hlsl-intrinsics.json"
        ))),
        ShadingLanguage::Glsl => ShaderSymbolList::parse_from_json(String::from(include_str!(
            "intrinsics/glsl-intrinsics.json"
        ))),
    }
}

// SCOPES

// How to affect scope to a symbol ?
// Vec<String> = ["", "fibonnaci"] // Could mean global / fibonnaci, but would require to parse header of scope... No trivial.
// Scope = {start:, end:,} // Store an optional scope range computed on symbol detection, found by iterating on scopes vec.
//      /-> Would have a scope [0, 2, 35, 98] instead that are index in scope range.
//          This mean all symbol in scope 0, 2, 35 & 98 arer compatibles.
// { // Scope 1
//  struct Value { // Scope 5
//      uint oui;
//  }
// }
// scope of Value == [1] (could have an owning scope (such as 5), this way, when dot is pressed on data type with owning scope, read values in this scope (and methods !))
// scope of oui = [1, 5]

pub(super) trait SymbolFilter {
    fn filter_symbols(&self, shader_symbols: &mut ShaderSymbolList, file_name: &String);
}

pub(super) trait SymbolParser {
    fn get_symbol_type(&self) -> ShaderSymbolType;
    // Return the regex for the type.
    fn get_capture_regex(&self) -> Option<Regex>;
    // Parse the result of the capture.
    fn parse_capture(
        &self,
        capture: Captures,
        shader_content: &String,
        path: &Path,
        scopes: &Vec<ShaderScope>,
    ) -> ShaderSymbol;
}

// This class should parse a file with a given position & return available symbols.
// It should even return all available symbols aswell as scopes, that are then recomputed
pub struct SymbolProvider {
    declarations: Vec<Box<dyn SymbolParser>>,
    filters: Vec<Box<dyn SymbolFilter>>,
    shading_language: ShadingLanguage,
}

impl SymbolProvider {
    pub fn glsl() -> Self {
        Self {
            declarations: vec![
                Box::new(GlslFunctionParser {}),
                Box::new(GlslStructParser {}),
                Box::new(GlslMacroParser {}),
                Box::new(GlslVariableParser {}),
            ],
            filters: vec![Box::new(GlslVersionFilter {}), Box::new(GlslStageFilter {})],
            shading_language: ShadingLanguage::Glsl,
        }
    }
    pub fn hlsl() -> Self {
        Self {
            declarations: vec![],
            filters: vec![],
            shading_language: ShadingLanguage::Hlsl,
        }
    }
    pub fn wgsl() -> Self {
        Self {
            declarations: vec![],
            filters: vec![],
            shading_language: ShadingLanguage::Wgsl,
        }
    }
    fn find_dependencies(
        include_handler: &mut IncludeHandler,
        shader_content: &String,
    ) -> Vec<(String, PathBuf)> {
        let include_regex = Regex::new("\\#include\\s+\"([\\w\\s\\\\/\\.\\-]+)\"").unwrap();
        let dependencies_paths: Vec<&str> = include_regex
            .captures_iter(&shader_content)
            .map(|c| c.get(1).unwrap().as_str())
            .collect();

        let mut dependencies = dependencies_paths
            .iter()
            .filter_map(|dependency| include_handler.search_in_includes(Path::new(dependency)))
            .collect::<Vec<(String, PathBuf)>>();

        let mut recursed_dependencies = Vec::new();
        for dependency in &dependencies {
            recursed_dependencies
                .append(&mut Self::find_dependencies(include_handler, &dependency.0));
        }
        recursed_dependencies.append(&mut dependencies);

        recursed_dependencies
    }
    pub(super) fn compute_scope_stack(
        position: &ShaderPosition,
        scopes: &Vec<ShaderScope>,
    ) -> Vec<ShaderScope> {
        // Find scope which are in range for given position & return them.
        let valid_scopes: Vec<&ShaderScope> =
            scopes.iter().filter(|s| s.is_in_range(&position)).collect();
        valid_scopes.iter().map(|s| (*s).clone()).collect()
    }
    fn compute_scopes(shader_content: &String, file_path: &Path) -> Vec<ShaderScope> {
        let mut scope_stack = Vec::new();
        let mut scopes = Vec::new();
        for (index, char) in shader_content.char_indices() {
            match char {
                '{' => scope_stack.push(index),
                '}' => scopes.push(ShaderScope {
                    start: ShaderPosition::from_pos(
                        &shader_content,
                        scope_stack.pop().unwrap_or(index),
                        file_path,
                    ),
                    end: ShaderPosition::from_pos(&shader_content, index, file_path),
                }),
                _ => {}
            }
        }
        scopes.sort_by(|a, b| a.start.cmp(&b.start));
        scopes
    }
    fn capture_into(
        shader_symbols: &mut Vec<ShaderSymbol>,
        parser: &dyn SymbolParser,
        shader_content: &String,
        file_path: &Path,
        scopes: &Vec<ShaderScope>,
    ) {
        match parser.get_capture_regex() {
            Some(regex) => {
                for capture in regex.captures_iter(&shader_content) {
                    shader_symbols.push(parser.parse_capture(
                        capture,
                        shader_content,
                        file_path,
                        scopes,
                    ));
                }
            }
            None => {} // Dont capture
        }
    }
    pub fn capture(
        &self,
        shader_content: &String,
        file_path: &Path,
        params: &ValidationParams,
        position: Option<ShaderPosition>
    ) -> ShaderSymbolList {
        let mut shader_symbols = get_default_shader_completion(self.shading_language);
        let mut handler = IncludeHandler::new(file_path, params.includes.clone());
        let mut dependencies = Self::find_dependencies(&mut handler, &shader_content);
        dependencies.push((shader_content.clone(), file_path.into()));

        for (dependency_content, dependency_path) in dependencies {
            let scopes = Self::compute_scopes(&dependency_content, &dependency_path);
            for declaration in &self.declarations {
                Self::capture_into(
                    match declaration.get_symbol_type() {
                        ShaderSymbolType::Types => &mut shader_symbols.types,
                        ShaderSymbolType::Constants => &mut shader_symbols.constants,
                        ShaderSymbolType::Variables => &mut shader_symbols.variables,
                        ShaderSymbolType::Functions => &mut shader_symbols.functions,
                    },
                    declaration.as_ref(),
                    &dependency_content,
                    &dependency_path,
                    &scopes,
                );
            }
        }
        // Dirty temp hack until completion is OK on HLSL / WGSL
        if self.shading_language == ShadingLanguage::Glsl {
            for define in &params.defines {
                shader_symbols.constants.push(ShaderSymbol {
                    label: define.0.clone(),
                    description: "Preprocessor macro".into(),
                    version: "".into(),
                    stages: Vec::new(),
                    link: None,
                    signature: None,
                    ty: None,
                    position: None,
                    scope_stack: None,
                });
            }
        }
        // Should be run directly on symbol add.
        let file_name = file_path.file_name().unwrap().to_string_lossy().to_string();
        for filter in &self.filters {
            filter.filter_symbols(&mut shader_symbols, &file_name);
        }

        // Filter symbol scope & position
        match position {
            Some(cursor_position) => {
                // Ensure symbols are already defined at pos
                let filter_position = |shader_symbol : &ShaderSymbol| -> bool {
                    match &shader_symbol.scope_stack {
                        Some(scope) => if scope.is_empty() {
                            true // Global space
                        } else {
                            match &shader_symbol.position {
                                Some(pos) => if pos.line == cursor_position.line {
                                    cursor_position.pos > pos.pos
                                } else {
                                    cursor_position.line > pos.line
                                }, 
                                None => true, // intrinsics
                            }
                        },
                        None => true, // Global space
                    }
                };
                // Ensure symbols are in scope
                let filter_scope = |shader_symbol : &ShaderSymbol| -> bool {
                    match &shader_symbol.position {
                        Some(symbol_position) => if symbol_position.file_path == file_path {
                            // If we are in main file, check if scope in range.
                            match &shader_symbol.scope_stack {
                                Some(symbol_scope_stack) => {
                                    for symbol_scope in symbol_scope_stack {
                                        if !symbol_scope.is_in_range(&cursor_position) {
                                            return false;
                                        }
                                    }
                                    true
                                },
                                None => true,
                            }
                        } else {
                            // If we are not in main file, only show whats in global scope.
                            match &shader_symbol.scope_stack {
                                Some(symbol_scope_stack) => symbol_scope_stack.is_empty(), // Global scope or inaccessible
                                None => true,
                            }
                        },
                        None => true,
                    }
                };
                let filter_all = |shader_symbols : &ShaderSymbol| -> bool {
                    filter_position(shader_symbols) && filter_scope(shader_symbols)
                };
                ShaderSymbolList {
                    functions: shader_symbols.functions.into_iter().filter(filter_all).collect(),
                    types: shader_symbols.types.into_iter().filter(filter_all).collect(),
                    constants: shader_symbols.constants.into_iter().filter(filter_all).collect(),
                    variables: shader_symbols.variables.into_iter().filter(filter_all).collect(),
                }
            },
            None => shader_symbols,
        }
    }
}
