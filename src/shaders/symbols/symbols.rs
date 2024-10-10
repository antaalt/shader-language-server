use std::{
    cmp::Ordering, path::{Path, PathBuf}
};

use regex::Regex;
use serde::{Deserialize, Serialize};
use tree_sitter::Parser;

use crate::shaders::{
    include::IncludeHandler,
    shader::{ShaderStage, ShadingLanguage},
    validator::validator::ValidationParams,
};

use super::{
    glsl_filter::{GlslStageFilter, GlslVersionFilter}, parser::SymbolParser,
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

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ShaderLabelSignature {
    pub label: String,
    pub description: String,
    pub signature: ShaderSignature,
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
    pub fn new(file_path: PathBuf, line: u32, pos: u32) -> Self {
        Self {
            file_path,
            line,
            pos,
        }
    }
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
pub struct ShaderRange {
    pub start: ShaderPosition,
    pub end: ShaderPosition,
}

/*#[derive(Debug, Default, Clone)]
pub struct ShaderScope {
    pub range: ShaderRange,
    pub depth: u32,
}*/
pub type ShaderScope = ShaderRange;

impl ShaderRange {
    pub fn new(start: ShaderPosition, end: ShaderPosition) -> Self {
        Self { start, end }
    }
    pub fn contain_bounds(&self, position: &ShaderRange) -> bool {
        self.contain(&position.start) && self.contain(&position.end)
    }
    pub fn contain(&self, position: &ShaderPosition) -> bool {
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
                position.pos >= self.start.pos && position.pos < self.end.pos
            } else if position.line == self.start.line && position.line < self.end.line {
                position.pos >= self.start.pos
            } else if position.line == self.end.line && position.line > self.start.line {
                position.pos <= self.end.pos
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
    pub label: String,
    pub signature: ShaderSignature,
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
            range: None, // Should have a position ?
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
            range: None, // Should have a position ?
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
    Link { target: ShaderPosition },
}

#[allow(non_snake_case)] // for JSON
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ShaderSymbol {
    pub label: String,            // Label for the item
    pub description: String,      // Description of the item
    pub version: String,          // Minimum version required for the item.
    pub stages: Vec<ShaderStage>, // Shader stages of the item
    pub link: Option<String>,     // Link to some external documentation
    pub data: ShaderSymbolData,   // Data for the variable
    // Runtime info. No serialization.
    #[serde(skip)]
    pub range: Option<ShaderRange>, // Range of symbol in shader
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
    pub fn find_symbols(&self, label: String) -> Vec<ShaderSymbol> {
        self.iter()
            .map(|e| {
                e.0.iter()
                    .filter_map(|e| if e.label == label {
                        Some(e.clone())
                    } else {
                        None
                    })
                    .collect::<Vec<ShaderSymbol>>()
            })
            .collect::<Vec<Vec<ShaderSymbol>>>()
            .concat()
    }
    pub fn find_symbol(&self, label: String) -> Option<ShaderSymbol> {
        for symbol_list in self.iter() {
            match symbol_list.0.iter().find(|e| e.label == label) {
                Some(symbol) => return Some(symbol.clone()),
                None => {}
            }
        }
        None
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
    pub fn append(&mut self, shader_symbol_list: ShaderSymbolList) {
        let mut shader_symbol_list_mut = shader_symbol_list;
        self.functions.append(&mut shader_symbol_list_mut.functions);
        self.variables.append(&mut shader_symbol_list_mut.variables);
        self.constants.append(&mut shader_symbol_list_mut.constants);
        self.types.append(&mut shader_symbol_list_mut.types);
        self.keywords.append(&mut shader_symbol_list_mut.keywords);
    }
    pub fn iter(&self) -> ShaderSymbolListIterator {
        ShaderSymbolListIterator {
            list: self,
            ty: Some(ShaderSymbolType::Types), // First one
        }
    }
    pub fn list_all(&self) -> Vec<ShaderSymbol> {
        self.iter()
            .map(|l| l.0.clone())
            .collect::<Vec<Vec<ShaderSymbol>>>()
            .concat()
    }
    pub fn filter_scoped_symbol(&self, cursor_position: ShaderPosition) -> ShaderSymbolList {
        // Ensure symbols are already defined at pos
        let filter_position = |shader_symbol: &ShaderSymbol| -> bool {
            match &shader_symbol.scope_stack {
                Some(scope) => {
                    if scope.is_empty() {
                        true // Global space
                    } else {
                        match &shader_symbol.range {
                            Some(range) => {
                                if range.start.line == cursor_position.line {
                                    cursor_position.pos > range.start.pos
                                } else {
                                    cursor_position.line > range.start.line
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
            match &shader_symbol.range {
                Some(symbol_range) => {
                    if symbol_range.start.file_path == cursor_position.file_path {
                        // If we are in main file, check if scope in range.
                        match &shader_symbol.scope_stack {
                            Some(symbol_scope_stack) => {
                                for symbol_scope in symbol_scope_stack {
                                    if !symbol_scope.contain(&cursor_position) {
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
        // TODO: should add a filter for when multiple same definition: pick latest (shadowing)
        let filter_all = |shader_symbols: &ShaderSymbol| -> Option<ShaderSymbol> {
            if filter_position(shader_symbols) && filter_scope(shader_symbols) {
                Some(shader_symbols.clone())
            } else {
                None
            }
        };
        ShaderSymbolList {
            functions: self
                .functions
                .iter()
                .filter_map(filter_all)
                .collect(),
            types: self
                .types
                .iter()
                .filter_map(filter_all)
                .collect(),
            constants: self
                .constants
                .iter()
                .filter_map(filter_all)
                .collect(),
            variables: self
                .variables
                .iter()
                .filter_map(filter_all)
                .collect(),
            keywords: self
                .keywords
                .iter()
                .filter_map(filter_all)
                .collect(),
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
            ShaderSymbolData::Link { target } => format!("\"{}\":{}:{}", self.label, target.line, target.pos),
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

// This class should parse a file with a given position & return available symbols.
// It should even return all available symbols aswell as scopes, that are then recomputed
pub struct SymbolProvider {
    shader_intrinsics: ShaderSymbolList,
    symbol_parser: SymbolParser,
    filters: Vec<Box<dyn SymbolFilter>>,
}

impl SymbolProvider {
    pub fn glsl() -> Self {
        Self {
            symbol_parser: SymbolParser::glsl(),
            shader_intrinsics: parse_default_shader_intrinsics(ShadingLanguage::Glsl),
            filters: vec![Box::new(GlslVersionFilter {}), Box::new(GlslStageFilter {})],
        }
    }
    pub fn hlsl() -> Self {
        Self {
            symbol_parser: SymbolParser::hlsl(),
            shader_intrinsics: parse_default_shader_intrinsics(ShadingLanguage::Hlsl),
            filters: vec![],
        }
    }
    pub fn wgsl() -> Self {
        Self {
            symbol_parser: SymbolParser::wgsl(),
            shader_intrinsics: parse_default_shader_intrinsics(ShadingLanguage::Wgsl),
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

    // Get all symbols including dependencies.
    pub fn get_all_symbols(
        &mut self,
        shader_content: &String,
        file_path: &Path,
        params: &ValidationParams,
    ) -> ShaderSymbolList {
        let mut shader_symbols = self.shader_intrinsics.clone();
        let mut handler = IncludeHandler::new(file_path, params.includes.clone());
        let mut dependencies = Self::find_dependencies(&mut handler, &shader_content);
        dependencies.push((shader_content.clone(), file_path.into()));

        for (dependency_content, dependency_path) in dependencies {
            shader_symbols.append(self.symbol_parser.query_local_symbols(
                &dependency_path,
                &dependency_content,
            ));
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
                range: None,
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
    pub fn get_word_range_at_position(&mut self, shader_content: &String, file_path: &Path, position: ShaderPosition) -> Option<(String, ShaderRange)> {
        self.symbol_parser.find_label_at_position(shader_content, file_path, position)
    }
}
