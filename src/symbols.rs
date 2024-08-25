use std::path::{Path, PathBuf};

use regex::{Captures, Regex};

use crate::{
    common::{
        get_shader_position, ShaderParameter, ShaderPosition, ShaderScope, ShaderSignature,
        ShaderSymbol, ShaderSymbolList, ShaderSymbolType,
    },
    include::IncludeHandler,
};

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

// This class should parse a file with a given position & return available symbols.
// It should even return all available symbols aswell as scopes, that are then recomputed
pub struct SymbolProvider {
    declarations: Vec<Box<dyn DeclarationParser>>,
}

trait DeclarationParser {
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
struct GlslFunctionParser {}
impl DeclarationParser for GlslFunctionParser {
    fn get_symbol_type(&self) -> ShaderSymbolType {
        ShaderSymbolType::Functions
    }
    fn get_capture_regex(&self) -> Option<Regex> {
        Some(
            Regex::new("\\b([\\w_]*)\\s+([\\w_-]*)[\\s]*\\(([\\s\\w,-\\[\\]]*)\\)[\\s]*\\{")
                .unwrap(),
        )
    }

    fn parse_capture(
        &self,
        capture: regex::Captures,
        shader_content: &String,
        path: &Path,
        scopes: &Vec<ShaderScope>,
    ) -> ShaderSymbol {
        let signature = capture.get(2).unwrap();
        let return_type = capture.get(1).unwrap().as_str();
        let function = capture.get(2).unwrap().as_str();
        let parameters: Vec<&str> = match capture.get(3) {
            Some(all_parameters) => {
                if all_parameters.is_empty() {
                    Vec::new()
                } else {
                    all_parameters.as_str().split(',').collect()
                }
            }
            None => Vec::new(),
        };
        let position = get_shader_position(&shader_content, signature.start(), path);

        ShaderSymbol {
            label: function.into(),
            description: "".into(),
            version: "".into(),
            stages: Vec::new(),
            link: None,
            signature: Some(ShaderSignature {
                returnType: return_type.to_string(),
                description: "".into(),
                parameters: parameters
                    .iter()
                    .map(|parameter| {
                        let values: Vec<&str> = parameter.split_whitespace().collect();
                        ShaderParameter {
                            ty: (*values.first().unwrap_or(&"void")).into(),
                            label: (*values.last().unwrap_or(&"type")).into(),
                            description: "".into(),
                        }
                    })
                    .collect(),
            }),
            ty: None,
            position: Some(position.clone()),
            scope_stack: Some(SymbolProvider::compute_scope_stack(&position, scopes)),
        }
    }
}
struct GlslStructParser {}
impl DeclarationParser for GlslStructParser {
    fn get_symbol_type(&self) -> ShaderSymbolType {
        ShaderSymbolType::Types
    }
    fn get_capture_regex(&self) -> Option<Regex> {
        Some(Regex::new("\\bstruct\\s+([\\w_-]+)\\s*\\{").unwrap())
    }

    fn parse_capture(
        &self,
        capture: Captures,
        shader_content: &String,
        path: &Path,
        scopes: &Vec<ShaderScope>,
    ) -> ShaderSymbol {
        let name = capture.get(1).unwrap();

        let position = get_shader_position(&shader_content, name.start(), path);
        ShaderSymbol {
            label: name.as_str().into(),
            description: "".into(),
            version: "".into(),
            stages: Vec::new(),
            link: None,
            signature: None,
            ty: None,
            position: Some(position.clone()),
            scope_stack: Some(SymbolProvider::compute_scope_stack(&position, scopes)),
        }
    }
}
struct GlslMacroParser {}
impl DeclarationParser for GlslMacroParser {
    fn get_symbol_type(&self) -> ShaderSymbolType {
        ShaderSymbolType::Constants
    }
    fn get_capture_regex(&self) -> Option<Regex> {
        Some(Regex::new("\\#define\\s+([\\w\\-]+)").unwrap())
    }

    fn parse_capture(
        &self,
        capture: Captures,
        shader_content: &String,
        path: &Path,
        scopes: &Vec<ShaderScope>,
    ) -> ShaderSymbol {
        let value = capture.get(1).unwrap();

        let position = get_shader_position(&shader_content, value.start(), path);
        ShaderSymbol {
            label: value.as_str().into(),
            description: "preprocessor macro".into(),
            version: "".into(),
            stages: Vec::new(),
            link: None,
            signature: None,
            ty: None,
            position: Some(position.clone()),
            scope_stack: Some(SymbolProvider::compute_scope_stack(&position, scopes)),
        }
    }
}
struct GlslVariableParser {}
impl DeclarationParser for GlslVariableParser {
    fn get_symbol_type(&self) -> ShaderSymbolType {
        ShaderSymbolType::Variables
    }

    fn get_capture_regex(&self) -> Option<Regex> {
        Some(Regex::new("\\b([\\w_]*)\\s+([\\w_-]*)\\s*[;=][^=]").unwrap())
    }

    fn parse_capture(
        &self,
        capture: Captures,
        shader_content: &String,
        path: &Path,
        scopes: &Vec<ShaderScope>,
    ) -> ShaderSymbol {
        let ty = capture.get(1).unwrap();
        let name = capture.get(2).unwrap();

        let position = get_shader_position(&shader_content, name.start(), path);
        ShaderSymbol {
            label: name.as_str().into(),
            description: "".into(),
            version: "".into(),
            stages: Vec::new(),
            link: None,
            signature: None,
            ty: Some(ty.as_str().into()),
            position: Some(position.clone()),
            scope_stack: Some(SymbolProvider::compute_scope_stack(&position, scopes)),
        }
    }
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
        }
    }
    pub fn hlsl() -> Self {
        Self {
            declarations: vec![],
        }
    }
    pub fn wgsl() -> Self {
        Self {
            declarations: vec![],
        }
    }
    fn compute_scope_stack(
        position: &ShaderPosition,
        scopes: &Vec<ShaderScope>,
    ) -> Vec<ShaderScope> {
        // Find scope which are in range for given position & return them.
        let valid_scopes: Vec<&ShaderScope> =
            scopes.iter().filter(|s| s.is_in_range(&position)).collect();
        let scope_stack = valid_scopes.iter().map(|s| (*s).clone()).collect();
        scope_stack
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
    fn compute_scopes(shader_content: &String, file_path: &Path) -> Vec<ShaderScope> {
        let mut scope_stack = Vec::new();
        let mut scopes = Vec::new();
        for (index, char) in shader_content.char_indices() {
            match char {
                '{' => scope_stack.push(index),
                '}' => scopes.push(ShaderScope {
                    start: get_shader_position(
                        &shader_content,
                        scope_stack.pop().unwrap_or(index),
                        file_path,
                    ),
                    end: get_shader_position(&shader_content, index, file_path),
                }),
                _ => {}
            }
        }
        scopes.sort_by(|a, b| a.start.cmp(&b.start));
        scopes
    }
    fn capture_into(
        shader_symbols: &mut Vec<ShaderSymbol>,
        parser: &dyn DeclarationParser,
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
        includes: Vec<String>,
        shader_symbols: &mut ShaderSymbolList,
    ) {
        let mut handler = IncludeHandler::new(file_path, includes.clone());
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
    }
}
