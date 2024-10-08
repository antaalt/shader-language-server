use std::{
    cmp::Ordering,
    path::{Path, PathBuf},
};

use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};

use crate::shaders::{
    include::IncludeHandler,
    shader::{ShaderStage, ShadingLanguage},
    validator::validator::ValidationParams,
};

use super::{
    glsl::{
        GlslFunctionParser, GlslMacroParser, GlslStageFilter, GlslStructParser, GlslVariableParser,
        GlslVersionFilter,
    },
    hlsl::{HlslFunctionParser, HlslMacroParser, HlslStructParser, HlslVariableParser},
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
        assert!(
            self.start.file_path == self.end.file_path,
            "Position start & end should have same value."
        );
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

pub type ShaderMember = ShaderParameter;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ShaderMethod {
    label: String,
    signature: ShaderSignature,
}

impl ShaderMember {
    pub fn as_symbol(&self) -> ShaderSymbol {
        ShaderSymbol {
            label: self.label.clone(),
            description: self.description.clone(),
            version: "".into(),
            stages: vec![],
            link: None,
            data: ShaderSymbolData::Variables {
                ty: self.ty.clone(),
            },
            position: None, // Should have a position ?
            scope_stack: None,
        }
    }
}

impl ShaderMethod {
    pub fn as_symbol(&self) -> ShaderSymbol {
        ShaderSymbol {
            label: self.label.clone(),
            description: self.signature.description.clone(),
            version: "".into(),
            stages: vec![],
            link: None,
            data: ShaderSymbolData::Functions {
                signatures: vec![self.signature.clone()],
            },
            position: None, // Should have a position ?
            scope_stack: None,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub enum ShaderSymbolData {
    #[default]
    None,
    Types {
        ty: String,
    },
    Struct {
        members: Vec<ShaderMember>,
        methods: Vec<ShaderMethod>,
    },
    Constants {
        ty: String,
        qualifier: String,
        value: String,
    },
    Variables {
        ty: String,
    },
    Functions {
        signatures: Vec<ShaderSignature>,
    },
    Keyword {},
}

#[allow(non_snake_case)] // for JSON
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ShaderSymbol {
    pub label: String,                    // Label for the item
    pub description: String,              // Description of the item
    pub version: String,                  // Minimum version required for the item.
    pub stages: Vec<ShaderStage>,         // Shader stages of the item
    pub link: Option<String>,             // Link to some external documentation
    pub data: ShaderSymbolData,           // Data for the variable
    pub position: Option<ShaderPosition>, // Position in shader
    #[serde(skip)]
    pub scope_stack: Option<Vec<ShaderScope>>, // Stack of declaration
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub enum ShaderSymbolType {
    #[default]
    Types,
    Constants,
    Variables,
    Functions,
    Keyword,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ShaderSymbolList {
    // Could use maps for faster search access (hover provider)
    pub types: Vec<ShaderSymbol>,
    pub constants: Vec<ShaderSymbol>,
    pub variables: Vec<ShaderSymbol>,
    pub functions: Vec<ShaderSymbol>,
    pub keywords: Vec<ShaderSymbol>,
}

impl ShaderSymbolList {
    pub fn parse_from_json(file_content: String) -> ShaderSymbolList {
        serde_json::from_str::<ShaderSymbolList>(&file_content)
            .expect("Failed to parse ShaderSymbolList")
    }
    pub fn find_symbols(&self, label: String) -> Vec<&ShaderSymbol> {
        self.iter()
            .map(|e| {
                e.0.iter()
                    .filter(|e| e.label == label)
                    .collect::<Vec<&ShaderSymbol>>()
            })
            .collect::<Vec<Vec<&ShaderSymbol>>>()
            .concat()
    }
    pub fn find_variable_symbol(&self, label: &String) -> Option<ShaderSymbol> {
        self.variables
            .iter()
            .find(|s| s.label == *label)
            .map(|s| s.clone())
    }
    pub fn find_type_symbol(&self, label: &String) -> Option<ShaderSymbol> {
        self.types
            .iter()
            .find(|s| s.label == *label)
            .map(|s| s.clone())
    }
    pub fn iter(&self) -> ShaderSymbolListIterator {
        ShaderSymbolListIterator {
            list: self,
            ty: Some(ShaderSymbolType::Types), // First one
        }
    }
}

pub struct ShaderSymbolListIterator<'a> {
    list: &'a ShaderSymbolList,
    ty: Option<ShaderSymbolType>,
}

impl<'a> Iterator for ShaderSymbolListIterator<'a> {
    type Item = (&'a Vec<ShaderSymbol>, ShaderSymbolType);

    fn next(&mut self) -> Option<Self::Item> {
        match &self.ty {
            Some(ty) => match ty {
                ShaderSymbolType::Types => {
                    self.ty = Some(ShaderSymbolType::Constants);
                    Some((&self.list.types, ShaderSymbolType::Types))
                }
                ShaderSymbolType::Constants => {
                    self.ty = Some(ShaderSymbolType::Variables);
                    Some((&self.list.constants, ShaderSymbolType::Constants))
                }
                ShaderSymbolType::Variables => {
                    self.ty = Some(ShaderSymbolType::Functions);
                    Some((&self.list.variables, ShaderSymbolType::Variables))
                }
                ShaderSymbolType::Functions => {
                    self.ty = Some(ShaderSymbolType::Keyword);
                    Some((&self.list.functions, ShaderSymbolType::Functions))
                }
                ShaderSymbolType::Keyword => {
                    self.ty = None;
                    Some((&self.list.keywords, ShaderSymbolType::Keyword))
                }
            },
            None => None,
        }
    }
}

pub struct ShaderSymbolListIntoIterator {
    list: ShaderSymbolList,
    ty: Option<ShaderSymbolType>,
}
impl Iterator for ShaderSymbolListIntoIterator {
    type Item = (Vec<ShaderSymbol>, ShaderSymbolType);

    fn next(&mut self) -> Option<Self::Item> {
        match self.ty.clone() {
            Some(ty) => match ty {
                ShaderSymbolType::Types => {
                    self.ty = Some(ShaderSymbolType::Constants);
                    Some((
                        std::mem::take(&mut self.list.types),
                        ShaderSymbolType::Types,
                    ))
                }
                ShaderSymbolType::Constants => {
                    self.ty = Some(ShaderSymbolType::Variables);
                    Some((
                        std::mem::take(&mut self.list.constants),
                        ShaderSymbolType::Constants,
                    ))
                }
                ShaderSymbolType::Variables => {
                    self.ty = Some(ShaderSymbolType::Functions);
                    Some((
                        std::mem::take(&mut self.list.variables),
                        ShaderSymbolType::Variables,
                    ))
                }
                ShaderSymbolType::Functions => {
                    self.ty = Some(ShaderSymbolType::Keyword);
                    Some((
                        std::mem::take(&mut self.list.functions),
                        ShaderSymbolType::Functions,
                    ))
                }
                ShaderSymbolType::Keyword => {
                    self.ty = None;
                    Some((
                        std::mem::take(&mut self.list.keywords),
                        ShaderSymbolType::Keyword,
                    ))
                }
            },
            None => None,
        }
    }
}

impl IntoIterator for ShaderSymbolList {
    type Item = (Vec<ShaderSymbol>, ShaderSymbolType);
    type IntoIter = ShaderSymbolListIntoIterator;

    fn into_iter(self) -> Self::IntoIter {
        ShaderSymbolListIntoIterator {
            list: self,
            ty: Some(ShaderSymbolType::Types), // First one
        }
    }
}

impl ShaderSymbol {
    pub fn format(&self) -> String {
        match &self.data {
            ShaderSymbolData::None => format!("Unknown {}", self.label.clone()),
            ShaderSymbolData::Types { ty } => format!("{}", ty), // ty == label
            ShaderSymbolData::Struct {
                members: _,
                methods: _,
            } => format!("struct {}", self.label.clone()),
            ShaderSymbolData::Constants {
                ty,
                qualifier,
                value,
            } => format!("{} {} {} = {};", qualifier, ty, self.label.clone(), value),
            ShaderSymbolData::Variables { ty } => format!("{} {}", ty, self.label),
            ShaderSymbolData::Functions { signatures } => signatures[0].format(&self.label), // TODO: append +1 symbol
            ShaderSymbolData::Keyword {} => format!("{}", self.label.clone()),
        }
    }
}

pub fn parse_default_shader_intrinsics(shading_language: ShadingLanguage) -> ShaderSymbolList {
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
    // Parse the members of the symbol (for class & structs only)
    /*fn parse_members_scope(
        &self,
        capture: Captures,
        shader_content: &String,
    ) -> Option<(usize, usize)>;*/
}

// This class should parse a file with a given position & return available symbols.
// It should even return all available symbols aswell as scopes, that are then recomputed
pub struct SymbolProvider {
    shader_intrinsics: ShaderSymbolList,
    declarations: Vec<Box<dyn SymbolParser>>,
    filters: Vec<Box<dyn SymbolFilter>>,
}

impl SymbolProvider {
    pub fn glsl() -> Self {
        Self {
            shader_intrinsics: parse_default_shader_intrinsics(ShadingLanguage::Glsl),
            declarations: vec![
                Box::new(GlslFunctionParser {}),
                Box::new(GlslStructParser {}),
                Box::new(GlslMacroParser {}),
                Box::new(GlslVariableParser {}),
            ],
            filters: vec![Box::new(GlslVersionFilter {}), Box::new(GlslStageFilter {})],
        }
    }
    pub fn hlsl() -> Self {
        Self {
            shader_intrinsics: parse_default_shader_intrinsics(ShadingLanguage::Hlsl),
            declarations: vec![
                Box::new(HlslFunctionParser {}),
                Box::new(HlslStructParser {}),
                Box::new(HlslMacroParser {}),
                Box::new(HlslVariableParser {}),
            ],
            filters: vec![],
        }
    }
    pub fn wgsl() -> Self {
        Self {
            shader_intrinsics: parse_default_shader_intrinsics(ShadingLanguage::Wgsl),
            declarations: vec![],
            filters: vec![],
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
    pub fn get_all_symbols_in_scope(
        &self,
        shader_content: &String,
        file_path: &Path,
        params: &ValidationParams,
        position: Option<ShaderPosition>,
    ) -> ShaderSymbolList {
        let shader_symbols = self.get_all_symbols(shader_content, file_path, params);
        // Filter symbol scope & position
        match position {
            Some(cursor_position) => {
                // Ensure symbols are already defined at pos
                let filter_position = |shader_symbol: &ShaderSymbol| -> bool {
                    match &shader_symbol.scope_stack {
                        Some(scope) => {
                            if scope.is_empty() {
                                true // Global space
                            } else {
                                match &shader_symbol.position {
                                    Some(pos) => {
                                        if pos.line == cursor_position.line {
                                            cursor_position.pos > pos.pos
                                        } else {
                                            cursor_position.line > pos.line
                                        }
                                    }
                                    None => true, // intrinsics
                                }
                            }
                        }
                        None => true, // Global space
                    }
                };
                // Ensure symbols are in scope
                let filter_scope = |shader_symbol: &ShaderSymbol| -> bool {
                    match &shader_symbol.position {
                        Some(symbol_position) => {
                            if symbol_position.file_path == file_path {
                                // If we are in main file, check if scope in range.
                                match &shader_symbol.scope_stack {
                                    Some(symbol_scope_stack) => {
                                        for symbol_scope in symbol_scope_stack {
                                            if !symbol_scope.is_in_range(&cursor_position) {
                                                return false;
                                            }
                                        }
                                        true
                                    }
                                    None => true,
                                }
                            } else {
                                // If we are not in main file, only show whats in global scope.
                                match &shader_symbol.scope_stack {
                                    Some(symbol_scope_stack) => symbol_scope_stack.is_empty(), // Global scope or inaccessible
                                    None => true,
                                }
                            }
                        }
                        None => true,
                    }
                };
                let filter_all = |shader_symbols: &ShaderSymbol| -> bool {
                    filter_position(shader_symbols) && filter_scope(shader_symbols)
                };
                ShaderSymbolList {
                    functions: shader_symbols
                        .functions
                        .into_iter()
                        .filter(filter_all)
                        .collect(),
                    types: shader_symbols
                        .types
                        .into_iter()
                        .filter(filter_all)
                        .collect(),
                    constants: shader_symbols
                        .constants
                        .into_iter()
                        .filter(filter_all)
                        .collect(),
                    variables: shader_symbols
                        .variables
                        .into_iter()
                        .filter(filter_all)
                        .collect(),
                    keywords: shader_symbols
                        .keywords
                        .into_iter()
                        .filter(filter_all)
                        .collect(),
                }
            }
            None => shader_symbols,
        }
    }
    pub fn get_all_symbols(
        &self,
        shader_content: &String,
        file_path: &Path,
        params: &ValidationParams,
    ) -> ShaderSymbolList {
        let mut shader_symbols = self.shader_intrinsics.clone();
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
                        ShaderSymbolType::Keyword => &mut shader_symbols.keywords,
                    },
                    declaration.as_ref(),
                    &dependency_content,
                    &dependency_path,
                    &scopes,
                );
            }
        }
        // Add custom macros to symbol list.
        for define in &params.defines {
            shader_symbols.constants.push(ShaderSymbol {
                label: define.0.clone(),
                description: format!("Preprocessor macro (value: {})", define.1),
                version: "".into(),
                stages: Vec::new(),
                link: None,
                data: ShaderSymbolData::Constants {
                    ty: "".into(),
                    qualifier: "".into(),
                    value: define.1.clone(),
                },
                position: None,
                scope_stack: None,
            });
        }
        // Should be run directly on symbol add.
        let file_name = file_path.file_name().unwrap().to_string_lossy().to_string();
        for filter in &self.filters {
            filter.filter_symbols(&mut shader_symbols, &file_name);
        }
        shader_symbols
    }
    /*pub fn get_type_symbols(&self, symbol: &ShaderSymbol) -> ShaderSymbolList {
        let mut global_shader_symbol_list = ShaderSymbolList::default();

        match &symbol.position {
            Some(pos) => {
                match std::fs::read_to_string(&pos.file_path) {
                    Ok(content) => {
                        for declaration in &self.declarations {
                            match declaration.get_capture_regex() {
                                Some(regex) => {
                                    let scopes = Self::compute_scopes(&content, &pos.file_path);
                                    for capture in regex.captures_iter(&content) {
                                        match declaration.parse_members_scope(capture, &content) {
                                            Some((start, end)) => {
                                                let slice = &content[start..end];
                                                // All capture to perform inside scope.
                                                for parser in &self.class_declaration {
                                                    match parser.get_capture_regex() {
                                                        Some(regex) => {
                                                            for capture in
                                                                regex.captures_iter(slice)
                                                            {
                                                                let list = match parser.get_symbol_type() {
                                                                    ShaderSymbolType::Types => &mut global_shader_symbol_list.types,
                                                                    ShaderSymbolType::Constants => &mut global_shader_symbol_list.constants,
                                                                    ShaderSymbolType::Variables => &mut global_shader_symbol_list.variables,
                                                                    ShaderSymbolType::Functions => &mut global_shader_symbol_list.functions,
                                                                    ShaderSymbolType::Keyword => &mut global_shader_symbol_list.keywords,
                                                                };
                                                                let symbol = parser.parse_capture(
                                                                    capture,
                                                                    &content,
                                                                    &pos.file_path,
                                                                    &scopes,
                                                                );
                                                                // TODO: Reoffset position correctly.
                                                                list.push(symbol);
                                                            }
                                                        }
                                                        None => {}
                                                    };
                                                }
                                            }
                                            None => {}
                                        }
                                    }
                                }
                                None => {} // Dont capture
                            }
                        }
                    }
                    Err(_) => {}
                }
            }
            None => {}
        }
        global_shader_symbol_list
    }*/
}
