use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::str::FromStr;

use crate::shaders::shader::{ShadingLanguage};
use crate::shaders::symbols::symbols::ShaderSymbol;
use crate::shaders::validator::validator::{ValidationParams, Validator};
#[cfg(not(target_os = "wasi"))]
use crate::shaders::validator::dxc::Dxc;
use crate::shaders::validator::glslang::Glslang;
use crate::shaders::include::Dependencies;
use crate::shaders::validator::naga::Naga;
use crate::shaders::shader_error::{ShaderErrorSeverity, ValidatorError};
use log::{debug, error, info, warn};
use lsp_types::notification::{
    DidChangeConfiguration, DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument,
    DidSaveTextDocument, Notification,
};
use lsp_types::request::{
    Completion, DocumentDiagnosticRequest, GotoDefinition, HoverRequest, Request,
    SignatureHelpRequest, WorkspaceConfiguration,
};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails,
    CompletionOptionsCompletionItem, CompletionParams, CompletionResponse, ConfigurationParams,
    Diagnostic, DidChangeConfigurationParams, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentDiagnosticParams, DocumentDiagnosticReport, DocumentDiagnosticReportResult,
    FullDocumentDiagnosticReport, GotoDefinitionParams, GotoDefinitionResponse, Hover,
    HoverContents, HoverParams, HoverProviderCapability, MarkupContent, ParameterInformation,
    ParameterLabel, Position, PublishDiagnosticsParams, RelatedFullDocumentDiagnosticReport,
    SignatureHelp, SignatureHelpOptions, SignatureHelpParams, SignatureInformation,
    TextDocumentItem, TextDocumentSyncKind, Url, WorkDoneProgressOptions,
};
use lsp_types::{InitializeParams, ServerCapabilities};

use lsp_server::{Connection, ErrorCode, IoThreads, Message, RequestId, Response};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[allow(non_snake_case)]
#[derive(Debug, Serialize, Deserialize)]
struct ServerConfig {
    autocomplete: bool,
    includes: Vec<String>,
    defines: HashMap<String, String>,
    validateOnType: bool,
    validateOnSave: bool,
    severity: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            autocomplete: true,
            includes: Vec::new(),
            defines: HashMap::new(),
            validateOnType: true,
            validateOnSave: true,
            severity: ShaderErrorSeverity::Hint.to_string(),
        }
    }
}

struct ServerFileCache {
    shading_language: ShadingLanguage,
    content: String,            // Store content on change as its not on disk.
    dependencies: Dependencies, // Store dependencies to link changes.
}

struct ServerLanguage {
    connection: Connection,
    io_threads: Option<IoThreads>,
    watched_files: HashMap<Url, ServerFileCache>,
    request_id: i32,
    request_callbacks: HashMap<RequestId, fn(&mut ServerLanguage, Value)>,
    config: ServerConfig,
    validators: HashMap<ShadingLanguage, Box<dyn Validator>>,
}

impl ServerLanguage {
    pub fn new() -> Self {
        // Create the transport. Includes the stdio (stdin and stdout) versions but this could
        // also be implemented to use sockets or HTTP.
        let (connection, io_threads) = Connection::stdio();

        // Create validators.
        let mut validators: HashMap<ShadingLanguage, Box<dyn Validator>> = HashMap::new();
        validators.insert(ShadingLanguage::Wgsl, Box::new(Naga::new()));
        #[cfg(target_os = "wasi")]
        validators.insert(ShadingLanguage::Hlsl, Box::new(Glslang::hlsl()));
        #[cfg(not(target_os = "wasi"))]
        validators.insert(
            ShadingLanguage::Hlsl,
            Box::new(Dxc::new().expect("Failed to create DXC")),
        );
        validators.insert(ShadingLanguage::Glsl, Box::new(Glslang::glsl()));

        // Run the server and wait for the two threads to end (typically by trigger LSP Exit event).
        Self {
            connection,
            io_threads: Some(io_threads),
            watched_files: HashMap::new(),
            request_id: 0,
            request_callbacks: HashMap::new(),
            config: ServerConfig::default(),
            validators: validators,
        }
    }
    pub fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
        let server_capabilities = serde_json::to_value(&ServerCapabilities {
            text_document_sync: Some(lsp_types::TextDocumentSyncCapability::Kind(
                TextDocumentSyncKind::FULL,
            )),
            completion_provider: Some(lsp_types::CompletionOptions {
                resolve_provider: None, // For more detailed data
                completion_item: Some(CompletionOptionsCompletionItem {
                    label_details_support: Some(true),
                }),
                ..Default::default()
            }),
            signature_help_provider: Some(SignatureHelpOptions {
                trigger_characters: Some(vec!["(".to_owned(), ",".to_owned()]),
                retrigger_characters: None,
                work_done_progress_options: WorkDoneProgressOptions {
                    work_done_progress: None,
                },
            }),
            hover_provider: Some(HoverProviderCapability::Simple(true)),
            definition_provider: Some(lsp_types::OneOf::Left(true)),
            ..Default::default()
        })?;
        let initialization_params = match self.connection.initialize(server_capabilities) {
            Ok(it) => it,
            Err(e) => {
                if e.channel_is_disconnected() {
                    self.io_threads.take().unwrap().join()?;
                }
                return Err(e.into());
            }
        };
        let client_initialization_params: InitializeParams =
            serde_json::from_value(initialization_params)?;
        debug!(
            "Received client params: {:#?}",
            client_initialization_params
        );

        self.request_configuration();

        return Ok(());
    }
    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
        loop {
            let msg_err = self.connection.receiver.recv();
            match msg_err {
                Ok(msg) => match msg {
                    Message::Request(req) => {
                        if self.connection.handle_shutdown(&req)? {
                            return Ok(());
                        }
                        self.on_request(req)?;
                    }
                    Message::Response(resp) => {
                        self.on_response(resp)?;
                    }
                    Message::Notification(not) => {
                        self.on_notification(not)?;
                    }
                },
                Err(_) => {
                    // Recv error means disconnected.
                    return Ok(());
                }
            }
        }
    }
    fn on_request(&mut self, req: lsp_server::Request) -> Result<(), serde_json::Error> {
        match req.method.as_str() {
            DocumentDiagnosticRequest::METHOD => {
                let params: DocumentDiagnosticParams = serde_json::from_value(req.params)?;
                debug!(
                    "Received document diagnostic request #{}: {:#?}",
                    req.id, params
                );
                match self.get_watched_file(&params.text_document.uri) {
                    Some(file) => {
                        let shading_language = file.shading_language;
                        let content = file.content.clone();
                        match self.recolt_diagnostic(
                            &params.text_document.uri,
                            shading_language,
                            content,
                        ) {
                            Ok(diagnostics) => {
                                for diagnostic in diagnostics {
                                    // TODO: clear URL
                                    if diagnostic.0 == params.text_document.uri {
                                        self.send_response::<DocumentDiagnosticRequest>(
                                            req.id.clone(),
                                            DocumentDiagnosticReportResult::Report(
                                                DocumentDiagnosticReport::Full(
                                                    RelatedFullDocumentDiagnosticReport {
                                                        related_documents: None, // TODO: data of other files.
                                                        full_document_diagnostic_report:
                                                            FullDocumentDiagnosticReport {
                                                                result_id: Some(req.id.to_string()),
                                                                items: diagnostic.1,
                                                            },
                                                    },
                                                ),
                                            ),
                                        )
                                    }
                                }
                            }
                            // Send empty report.
                            Err(error) => self.send_response_error(
                                req.id,
                                lsp_server::ErrorCode::InternalError,
                                error.to_string(),
                            ),
                        };
                    }
                    None => self.send_response_error(
                        req.id,
                        ErrorCode::InvalidParams,
                        "Requesting diagnostic on file that is not watched".to_string(),
                    ),
                }
            }
            GotoDefinition::METHOD => {
                let params: GotoDefinitionParams = serde_json::from_value(req.params)?;
                debug!("Received gotoDefinition request #{}: {:#?}", req.id, params);
                match self.get_watched_file(&params.text_document_position_params.text_document.uri)
                {
                    Some(file) => {
                        let uri = params.text_document_position_params.text_document.uri;
                        let shading_language = file.shading_language;
                        let content = file.content.clone();
                        let position = params.text_document_position_params.position;
                        match self.recolt_goto(&uri, shading_language, content, position) {
                            Ok(value) => self.send_response::<GotoDefinition>(req.id, value),
                            Err(err) => self.send_response_error(
                                req.id,
                                ErrorCode::InvalidParams,
                                format!("Failed to recolt signature : {:#?}", err),
                            ),
                        }
                    }
                    None => self.send_response_error(
                        req.id,
                        ErrorCode::InvalidParams,
                        "Requesting hover on file that is not watched".to_string(),
                    ),
                }
            }
            Completion::METHOD => {
                let params: CompletionParams = serde_json::from_value(req.params)?;
                debug!("Received completion request #{}: {:#?}", req.id, params);
                match self.get_watched_file(&params.text_document_position.text_document.uri) {
                    Some(file) => {
                        let shading_language = file.shading_language;
                        let content = file.content.clone();
                        match self.recolt_completion(
                            &params.text_document_position.text_document.uri,
                            shading_language,
                            content,
                            params.text_document_position.position,
                        ) {
                            Ok(value) => self.send_response::<Completion>(
                                req.id,
                                Some(CompletionResponse::Array(value)),
                            ),
                            Err(error) => self.send_response_error(
                                req.id,
                                lsp_server::ErrorCode::InternalError,
                                error.to_string(),
                            ),
                        }
                    }
                    None => self.send_response_error(
                        req.id,
                        ErrorCode::InvalidParams,
                        "Requesting diagnostic on file that is not watched".to_string(),
                    ),
                }
            }
            SignatureHelpRequest::METHOD => {
                let params: SignatureHelpParams = serde_json::from_value(req.params)?;
                debug!("Received completion request #{}: {:#?}", req.id, params);
                match self.get_watched_file(&params.text_document_position_params.text_document.uri)
                {
                    Some(file) => {
                        let uri = params.text_document_position_params.text_document.uri;
                        let shading_language = file.shading_language;
                        let content = file.content.clone();
                        let position = params.text_document_position_params.position;
                        match self.recolt_signature(&uri, shading_language, content, position) {
                            Ok(value) => self.send_response::<SignatureHelpRequest>(req.id, value),
                            Err(err) => self.send_response_error(
                                req.id,
                                ErrorCode::InvalidParams,
                                format!("Failed to recolt signature : {:#?}", err),
                            ),
                        }
                    }
                    None => self.send_response_error(
                        req.id,
                        ErrorCode::InvalidParams,
                        "Requesting signature on file that is not watched".to_string(),
                    ),
                }
            }
            HoverRequest::METHOD => {
                let params: HoverParams = serde_json::from_value(req.params)?;
                debug!("Received hover request #{}: {:#?}", req.id, params);
                match self.get_watched_file(&params.text_document_position_params.text_document.uri)
                {
                    Some(file) => {
                        let uri = params.text_document_position_params.text_document.uri;
                        let shading_language = file.shading_language;
                        let content = file.content.clone();
                        let position = params.text_document_position_params.position;
                        match self.recolt_hover(&uri, shading_language, content, position) {
                            Ok(value) => self.send_response::<HoverRequest>(req.id, value),
                            Err(err) => self.send_response_error(
                                req.id,
                                ErrorCode::InvalidParams,
                                format!("Failed to recolt signature : {:#?}", err),
                            ),
                        }
                    }
                    None => self.send_response_error(
                        req.id,
                        ErrorCode::InvalidParams,
                        "Requesting hover on file that is not watched".to_string(),
                    ),
                }
            }
            _ => {
                warn!("Received unhandled request: {:#?}", req);
            }
        }
        Ok(())
    }
    fn on_response(&mut self, response: lsp_server::Response) -> Result<(), serde_json::Error> {
        match self.request_callbacks.remove(&response.id) {
            Some(callback) => match response.result {
                Some(result) => callback(self, result),
                None => callback(self, serde_json::from_str("{}").unwrap()),
            },
            None => warn!("Received unhandled response: {:#?}", response),
        }
        Ok(())
    }
    fn on_notification(
        &mut self,
        notification: lsp_server::Notification,
    ) -> Result<(), serde_json::Error> {
        debug!("Received notification: {}", notification.method);
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let params: DidOpenTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                match self.watch_file(&params.text_document) {
                    Ok(lang) => {
                        self.update_watched_file_content(
                            &params.text_document.uri,
                            params.text_document.text.clone(),
                        );
                        self.publish_diagnostic(
                            &params.text_document.uri,
                            lang,
                            params.text_document.text,
                            Some(params.text_document.version),
                        );
                        debug!(
                            "Starting watching {:#?} file at {:#?}",
                            lang, params.text_document.uri
                        );
                    }
                    Err(()) => {
                        warn!(
                            "Received unhandled shading language : {:#?}",
                            params.text_document
                        );
                    }
                };
            }
            DidSaveTextDocument::METHOD => {
                let params: DidSaveTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                debug!(
                    "got did save text document: {:#?}",
                    params.text_document.uri
                );
                if self.config.validateOnSave {
                    match self.get_watched_file(&params.text_document.uri) {
                        Some(file) => {
                            let shading_language = file.shading_language;
                            let content = match params.text {
                                Some(value) => {
                                    self.update_watched_file_content(
                                        &params.text_document.uri,
                                        value.clone(),
                                    );
                                    value
                                }
                                None => file.content.clone(),
                            };
                            self.publish_diagnostic(
                                &params.text_document.uri,
                                shading_language,
                                content,
                                None,
                            )
                        }
                        None => error!(
                            "Trying to save watched file that is not watched : {}",
                            params.text_document.uri
                        ),
                    }
                }
            }
            DidCloseTextDocument::METHOD => {
                let params: DidCloseTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                debug!(
                    "got did close text document: {:#?}",
                    params.text_document.uri
                );
                self.clear_diagnostic(&params.text_document.uri);
                self.remove_watched_file(&params.text_document.uri);
            }
            DidChangeTextDocument::METHOD => {
                let params: DidChangeTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                debug!(
                    "got did change text document: {:#?}",
                    params.text_document.uri
                );
                if self.config.validateOnType {
                    match self.get_watched_file(&params.text_document.uri) {
                        Some(file) => {
                            let shading_language = file.shading_language;
                            for content in params.content_changes {
                                self.update_watched_file_content(
                                    &params.text_document.uri,
                                    content.text.clone(),
                                );
                                self.publish_diagnostic(
                                    &params.text_document.uri,
                                    shading_language,
                                    content.text,
                                    Some(params.text_document.version),
                                );
                            }
                        }
                        None => error!(
                            "Trying to change watched file that is not watched : {}",
                            params.text_document.uri
                        ),
                    }
                }
            }
            DidChangeConfiguration::METHOD => {
                let params: DidChangeConfigurationParams =
                    serde_json::from_value(notification.params)?;
                debug!("Received did change configuration document: {:#?}", params);
                // Here config received is empty. we need to request it to user.
                //let config : ServerConfig = serde_json::from_value(params.settings)?;
                self.request_configuration();
            }
            _ => {
                warn!("Received unhandled notification: {:#?}", notification);
            }
        }
        Ok(())
    }

    fn watch_file(&mut self, text_document: &TextDocumentItem) -> Result<ShadingLanguage, ()> {
        match ShadingLanguage::from_str(text_document.language_id.as_str()) {
            Ok(lang) => {
                let file_path = text_document
                    .uri
                    .to_file_path()
                    .expect("Failed to decode uri");
                match self.watched_files.insert(
                    text_document.uri.clone(),
                    ServerFileCache {
                        shading_language: lang,
                        content: std::fs::read_to_string(&file_path).expect("Failed to read file"),
                        dependencies: Dependencies::new(),
                    },
                ) {
                    Some(_) => {
                        error!(
                            "Adding a file to watch that is already watched: {}",
                            text_document.uri
                        )
                    }
                    None => {}
                }
                Ok(lang)
            }
            Err(()) => Err(()),
        }
    }
    fn update_watched_file_content(&mut self, uri: &Url, content: String) {
        match self.watched_files.get_mut(uri) {
            Some(file) => file.content = content,
            None => error!(
                "Trying to change content of file {} that is not watched.",
                uri
            ),
        };
    }
    fn update_watched_file_dependencies(&mut self, uri: &Url, dependencies: Dependencies) {
        match self.watched_files.get_mut(uri) {
            Some(file) => file.dependencies = dependencies,
            None => error!(
                "Trying to change dependencies of file {} that is not watched.",
                uri
            ),
        };
    }
    fn get_watched_file(&mut self, uri: &Url) -> Option<&ServerFileCache> {
        match self.watched_files.get(uri) {
            Some(file) => Some(file),
            None => None,
        }
    }
    #[allow(dead_code)]
    fn visit_watched_file<F: Fn(&ServerFileCache)>(&self, uri: &Url, callback: F) {
        match self.watched_files.get(uri) {
            Some(file) => callback(file),
            None => error!("Trying to visit file that is not watched : {}", uri),
        };
    }
    #[allow(dead_code)]
    fn visit_watched_file_mut<F: Fn(&mut ServerFileCache)>(&mut self, uri: &Url, callback: F) {
        match self.watched_files.get_mut(uri) {
            Some(file) => callback(file),
            None => error!("Trying to visit file that is not watched : {}", uri),
        };
    }
    fn remove_watched_file(&mut self, uri: &Url) {
        match self.watched_files.remove(&uri) {
            Some(_) => {}
            None => error!("Trying to remove file that is not watched : {}", uri),
        }
    }
    fn get_word_range_at_position(
        shader: String,
        position: Position,
    ) -> Option<(String, lsp_types::Range)> {
        // vscode getWordRangeAtPosition does something similar
        let reader = BufReader::new(shader.as_bytes());
        let line = reader
            .lines()
            .nth(position.line as usize)
            .expect("Text position is out of bounds")
            .expect("Could not read line");
        let regex =
            Regex::new("(-?\\d*\\.\\d\\w*)|([^\\`\\~\\!\\@\\#\\%\\^\\&\\*\\(\\)\\-\\=\\+\\[\\{\\]\\}\\\\|\\;\\:\\'\\\"\\,\\.<>\\/\\?\\s]+)").expect("Failed to init regex");
        for capture in regex.captures_iter(line.as_str()) {
            let word = capture.get(0).expect("Failed to get word");
            if position.character >= word.start() as u32 && position.character <= word.end() as u32
            {
                return Some((
                    line[word.start()..word.end()].into(),
                    lsp_types::Range::new(
                        lsp_types::Position::new(position.line, word.start() as u32),
                        lsp_types::Position::new(position.line, word.end() as u32),
                    ),
                ));
            }
        }
        None
    }
    fn get_function_parameter_at_position(
        shader: &String,
        position: Position,
    ) -> (Option<String>, Option<u32>) {
        let reader = BufReader::new(shader.as_bytes());
        let line = reader
            .lines()
            .nth(position.line as usize)
            .expect("Text position is out of bounds")
            .expect("Could not read line");
        // Check this regex is working for all lang.
        let regex =
            Regex::new("\\b([a-zA-Z_][a-zA-Z0-9_]*)(\\(.*?)(\\))").expect("Failed to init regex");
        for capture in regex.captures_iter(line.as_str()) {
            let file_name = capture.get(1).expect("Failed to get function name");
            let parenthesis = capture.get(2).expect("Failed to get paranthesis");
            let parameter_index = if position.character >= parenthesis.start() as u32
                && position.character <= parenthesis.end() as u32
            {
                let parameters = line[parenthesis.start()..parenthesis.end()].to_string();
                let parameters = parameters.split(',');
                let pos_in_parameters = position.character as usize - parenthesis.start();
                // Compute parameter index
                let mut parameter_index = 0;
                let mut parameter_offset = 0;
                for parameter in parameters {
                    parameter_offset += parameter.len() + 1; // Add 1 for removed comma
                    if parameter_offset > pos_in_parameters {
                        break;
                    }
                    parameter_index += 1;
                }
                Some(parameter_index)
            } else {
                None
            };
            if position.character >= file_name.start() as u32
                && position.character <= parenthesis.end() as u32
            {
                return (
                    Some(line[file_name.start()..file_name.end()].to_string()),
                    parameter_index,
                );
            }
        }
        // No signature
        (None, None)
    }
    fn recolt_signature(
        &mut self,
        uri: &Url,
        shading_language: ShadingLanguage,
        content: String,
        position: Position,
    ) -> Result<Option<SignatureHelp>, ValidatorError> {
        let function_parameter = Self::get_function_parameter_at_position(&content, position);
        debug!("Found requested func name {:?}", function_parameter);

        let file_path = uri
            .to_file_path()
            .expect(format!("Failed to convert {} to a valid path.", uri).as_str());
        let includes = self.config.includes.clone();
        let defines = self.config.defines.clone();
        let validator = self.get_validator(shading_language);
        match validator.get_shader_completion(
            content,
            &file_path,
            ValidationParams::new(includes, defines),
        ) {
            Ok(completion) => {
                let (shader_symbols, parameter_index): (Vec<&ShaderSymbol>, u32) =
                    if let (Some(function), Some(parameter_index)) = function_parameter {
                        (
                            completion
                                .functions
                                .iter()
                                .filter(|shader_symbol| shader_symbol.label == function)
                                .collect(),
                            parameter_index,
                        )
                    } else {
                        (Vec::new(), 0)
                    };
                let signatures: Vec<SignatureInformation> = shader_symbols
                    .iter()
                    .filter_map(|shader_symbol| {
                        if let Some(signature) = &shader_symbol.signature {
                            Some(SignatureInformation {
                                label: signature.format(shader_symbol.label.as_str()),
                                documentation: Some(lsp_types::Documentation::MarkupContent(
                                    MarkupContent {
                                        kind: lsp_types::MarkupKind::Markdown,
                                        value: shader_symbol.description.clone(),
                                    },
                                )),
                                parameters: Some(
                                    signature
                                        .parameters
                                        .iter()
                                        .map(|e| ParameterInformation {
                                            label: ParameterLabel::Simple(e.label.clone()),
                                            documentation: Some(
                                                lsp_types::Documentation::MarkupContent(
                                                    MarkupContent {
                                                        kind: lsp_types::MarkupKind::Markdown,
                                                        value: e.description.clone(),
                                                    },
                                                ),
                                            ),
                                        })
                                        .collect(),
                                ),
                                active_parameter: None,
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                if signatures.is_empty() {
                    debug!("No signature for symbol {:?} found", shader_symbols);
                    Ok(None)
                } else {
                    Ok(Some(SignatureHelp {
                        signatures: signatures,
                        active_signature: None,
                        active_parameter: Some(parameter_index), // TODO: check out of bounds.
                    }))
                }
            }
            Err(err) => Err(err),
        }
    }
    fn recolt_hover(
        &mut self,
        uri: &Url,
        shading_language: ShadingLanguage,
        content: String,
        position: Position,
    ) -> Result<Option<Hover>, ValidatorError> {
        let word_and_range = Self::get_word_range_at_position(content.clone(), position);
        match word_and_range {
            Some(word_and_range) => {
                let file_path = uri
                    .to_file_path()
                    .expect(format!("Failed to convert {} to a valid path.", uri).as_str());
                let includes = self.config.includes.clone();
                let defines = self.config.defines.clone();
                let validator = self.get_validator(shading_language);
                match validator.get_shader_completion(
                    content,
                    &file_path,
                    ValidationParams::new(includes, defines),
                ) {
                    Ok(completion) => {
                        let symbols = completion.find_symbols(word_and_range.0);
                        if symbols.is_empty() {
                            Ok(None)
                        } else {
                            let symbol = symbols[0];
                            let label = symbol.format();
                            let description = symbol.description.clone();
                            let link = match &symbol.link {
                                Some(link) => format!("[Online documentation]({})", link),
                                None => "".into(),
                            };
                            Ok(Some(Hover {
                                contents: HoverContents::Markup(MarkupContent {
                                    kind: lsp_types::MarkupKind::Markdown,
                                    value: format!(
                                        "```{}\n{}\n```\n{}{}\n\n{}",
                                        shading_language.to_string(),
                                        label,
                                        if symbols.len() > 1 {
                                            format!("(+{} symbol)\n\n", symbols.len() - 1)
                                        } else {
                                            "".into()
                                        },
                                        description,
                                        link
                                    ),
                                }),
                                range: Some(word_and_range.1),
                            }))
                        }
                    }
                    Err(err) => Err(err),
                }
            }
            None => Ok(None),
        }
    }
    fn recolt_goto(
        &mut self,
        uri: &Url,
        shading_language: ShadingLanguage,
        content: String,
        position: Position,
    ) -> Result<Option<GotoDefinitionResponse>, ValidatorError> {
        let word_and_range = Self::get_word_range_at_position(content.clone(), position);
        match word_and_range {
            Some(word_and_range) => {
                let file_path = uri
                    .to_file_path()
                    .expect(format!("Failed to convert {} to a valid path.", uri).as_str());
                let includes = self.config.includes.clone();
                let defines = self.config.defines.clone();
                let validator = self.get_validator(shading_language);
                match validator.get_shader_completion(
                    content,
                    &file_path,
                    ValidationParams::new(includes, defines),
                ) {
                    Ok(completion) => {
                        let symbols = completion.find_symbols(word_and_range.0);
                        if symbols.is_empty() {
                            Ok(None)
                        } else {
                            Ok(Some(GotoDefinitionResponse::Array(
                                symbols
                                    .iter()
                                    .filter_map(|symbol| match &symbol.position {
                                        Some(pos) => Some(lsp_types::Location {
                                            uri: Url::from_file_path(&pos.file_path)
                                                .expect("Failed to convert file path"),
                                            range: lsp_types::Range::new(
                                                lsp_types::Position::new(pos.line, pos.pos),
                                                lsp_types::Position::new(pos.line, pos.pos),
                                            ),
                                        }),
                                        None => None,
                                    })
                                    .collect(),
                            )))
                        }
                    }
                    Err(err) => Err(err),
                }
            }
            None => Ok(None),
        }
    }

    fn recolt_diagnostic(
        &mut self,
        uri: &Url,
        shading_language: ShadingLanguage,
        shader_source: String,
    ) -> Result<HashMap<Url, Vec<Diagnostic>>, ValidatorError> {
        // Skip non file uri.
        match uri.scheme() {
            "file" => {}
            _ => {
                return Err(ValidatorError::InternalErr(String::from(
                    "Cannot treat files without file scheme",
                )));
            }
        }
        let file_path = uri
            .to_file_path()
            .expect(format!("Failed to convert {} to a valid path.", uri).as_str());
        let includes = self.config.includes.clone();
        let defines = self.config.defines.clone();
        let validator = self.get_validator(shading_language);
        let clean_url = |url: &Url| -> Url {
            // Workaround issue with url encoded as &3a that break key comparison. Need to clean it.
            Url::from_file_path(url.to_file_path().unwrap()).unwrap()
        };
        match validator.validate_shader(
            shader_source,
            file_path.as_path(),
            ValidationParams::new(includes, defines),
        ) {
            Ok((diagnostic_list, dependencies)) => {
                self.update_watched_file_dependencies(uri, dependencies.clone());
                let mut diagnostics: HashMap<Url, Vec<Diagnostic>> = HashMap::new();
                for diagnostic in diagnostic_list.diagnostics {
                    let uri = match diagnostic.relative_path {
                        Some(relative_path) => {
                            let parent_path = file_path.parent().unwrap();
                            let absolute_path =
                                std::fs::canonicalize(parent_path.join(relative_path.clone()))
                                    .expect(
                                        format!(
                                            "Failed to canonicalize path from parent {} and {}",
                                            parent_path.display(),
                                            relative_path.display()
                                        )
                                        .as_str(),
                                    );
                            Url::from_file_path(&absolute_path).expect(
                                format!(
                                    "Failed to convert path {} to uri",
                                    absolute_path.display()
                                )
                                .as_str(),
                            )
                        }
                        None => clean_url(uri),
                    };
                    if diagnostic
                        .severity
                        .is_required(ShaderErrorSeverity::from(self.config.severity.clone()))
                    {
                        let diagnostic = Diagnostic {
                            range: lsp_types::Range::new(
                                lsp_types::Position::new(diagnostic.line - 1, diagnostic.pos),
                                lsp_types::Position::new(diagnostic.line - 1, diagnostic.pos),
                            ),
                            severity: Some(match diagnostic.severity {
                                ShaderErrorSeverity::Hint => lsp_types::DiagnosticSeverity::HINT,
                                ShaderErrorSeverity::Information => {
                                    lsp_types::DiagnosticSeverity::INFORMATION
                                }
                                ShaderErrorSeverity::Warning => {
                                    lsp_types::DiagnosticSeverity::WARNING
                                }
                                ShaderErrorSeverity::Error => lsp_types::DiagnosticSeverity::ERROR,
                            }),
                            message: diagnostic.error,
                            source: Some("shader-validator".to_string()),
                            ..Default::default()
                        };
                        match diagnostics.get_mut(&uri) {
                            Some(value) => value.push(diagnostic),
                            None => {
                                diagnostics.insert(uri, vec![diagnostic]);
                            }
                        };
                    }
                }
                let cleaned_uri = clean_url(uri);
                // Clear diagnostic if no errors.
                if diagnostics.get(&cleaned_uri).is_none() {
                    info!(
                        "Clearing diagnostic for main file {} (diags:{:?})",
                        cleaned_uri, diagnostics
                    );
                    diagnostics.insert(cleaned_uri.clone(), vec![]);
                }
                // Add empty diagnostics to dependencies without errors to clear them.
                dependencies.visit_dependencies(&mut |dep| {
                    let uri = Url::from_file_path(&dep).unwrap();
                    if diagnostics.get(&uri).is_none() {
                        info!(
                            "Clearing diagnostic for deps file {} (diags:{:?})",
                            uri, diagnostics
                        );
                        diagnostics.insert(uri, vec![]);
                    }
                });
                Ok(diagnostics)
            }
            Err(err) => Err(err),
        }
    }
    fn convert_completion_item(
        shading_language: ShadingLanguage,
        shader_symbol: ShaderSymbol,
        completion_kind: CompletionItemKind,
        variant_count: Option<u32>,
    ) -> CompletionItem {
        let doc_link = if let Some(link) = &shader_symbol.link {
            if !link.is_empty() {
                format!("\n[Online documentation]({})", link)
            } else {
                "".to_string()
            }
        } else {
            "".to_string()
        };
        let doc_signature = if let Some(signature) = &shader_symbol.signature {
            let parameters = signature
                .parameters
                .iter()
                .map(|p| format!("- `{} {}` {}", p.ty, p.label, p.description))
                .collect::<Vec<String>>();
            let parameters_markdown = if parameters.is_empty() {
                "".into()
            } else {
                format!("**Parameters:**\n\n{}", parameters.join("\n\n"))
            };
            format!(
                "\n**Return type:**\n\n`{}` {}\n\n{}",
                signature.returnType, signature.description, parameters_markdown
            )
        } else {
            "".to_string()
        };
        let position = if let Some(position) = &shader_symbol.position {
            format!(
                "{}:{}:{}",
                position
                    .file_path
                    .file_name()
                    .unwrap_or(OsStr::new("file"))
                    .to_string_lossy(),
                position.line,
                position.pos
            )
        } else {
            "".to_string()
        };
        let shading_language = shading_language.to_string();
        let description = {
            let mut description = shader_symbol.description.clone();
            let max_len = 500;
            if description.len() > max_len {
                description.truncate(max_len);
                description.push_str("...");
            }
            description
        };

        let signature = shader_symbol.format();
        CompletionItem {
            kind: Some(completion_kind),
            label: shader_symbol.label.clone(),
            detail: None,
            label_details: Some(CompletionItemLabelDetails {
                detail: None,
                description: match &shader_symbol.signature {
                    Some(sig) => Some(match variant_count {
                        Some(count) => if count > 1 {
                            format!("{} (+ {})", sig.format(shader_symbol.label.as_str()), count - 1)
                        } else {
                            sig.format(shader_symbol.label.as_str())
                        },
                        None => sig.format(shader_symbol.label.as_str()),
                    }),
                    None => None,
                },
            }),
            filter_text: Some(shader_symbol.label.clone()),
            documentation: Some(lsp_types::Documentation::MarkupContent(MarkupContent {
                kind: lsp_types::MarkupKind::Markdown,
                value: format!("```{shading_language}\n{signature}\n```\n{description}\n\n{doc_signature}\n\n{position}\n{doc_link}"),
            })),
            ..Default::default()
        }
    }
    fn recolt_completion(
        &mut self,
        uri: &Url,
        shading_language: ShadingLanguage,
        shader_source: String,
        _position: Position,
    ) -> Result<Vec<CompletionItem>, ValidatorError> {
        let file_path = uri
            .to_file_path()
            .expect(format!("Failed to convert {} to a valid path.", uri).as_str());
        let includes = self.config.includes.clone();
        let defines = self.config.defines.clone();
        let validator = self.get_validator(shading_language);
        match validator.get_shader_completion(
            shader_source,
            &file_path,
            ValidationParams::new(includes, defines),
        ) {
            Ok(symbols) => {
                let filter_symbols = |symbols: Vec<ShaderSymbol>| -> Vec<(ShaderSymbol, u32)> {
                    let mut set = HashMap::<String, (ShaderSymbol, u32)>::new();
                    for symbol in symbols {
                        match set.get_mut(&symbol.label) {
                            Some((_, count)) => *count += 1,
                            None => {
                                set.insert(symbol.label.clone(), (symbol, 1));
                            }
                        };
                    }
                    set.iter().map(|e| e.1.clone()).collect()
                };
                let mut items = Vec::<CompletionItem>::new();
                items.append(
                    &mut filter_symbols(symbols.functions)
                        .iter()
                        .map(|s| {
                            Self::convert_completion_item(
                                shading_language,
                                s.0.clone(),
                                CompletionItemKind::FUNCTION,
                                Some(s.1.clone()),
                            )
                        })
                        .collect(),
                );
                items.append(
                    &mut filter_symbols(symbols.constants)
                        .iter()
                        .map(|s| {
                            Self::convert_completion_item(
                                shading_language,
                                s.0.clone(),
                                CompletionItemKind::CONSTANT,
                                Some(s.1.clone()),
                            )
                        })
                        .collect(),
                );
                items.append(
                    &mut filter_symbols(symbols.variables)
                        .iter()
                        .map(|s| {
                            Self::convert_completion_item(
                                shading_language,
                                s.0.clone(),
                                CompletionItemKind::VARIABLE,
                                Some(s.1.clone()),
                            )
                        })
                        .collect(),
                );
                items.append(
                    &mut filter_symbols(symbols.types)
                        .iter()
                        .map(|s| {
                            Self::convert_completion_item(
                                shading_language,
                                s.0.clone(),
                                CompletionItemKind::TYPE_PARAMETER,
                                Some(s.1.clone()),
                            )
                        })
                        .collect(),
                );
                Ok(items)
            }
            Err(err) => Err(err),
        }
    }

    fn request_configuration(&mut self) {
        let config = ConfigurationParams {
            items: vec![lsp_types::ConfigurationItem {
                scope_uri: None,
                section: Some("shader-validator".to_owned()),
            }],
        };
        self.send_request::<WorkspaceConfiguration>(
            config,
            |server: &mut ServerLanguage, value: Value| {
                // Sent 1 item, received 1 in an array
                let mut parsed_config: Vec<ServerConfig> =
                    serde_json::from_value(value).expect("Failed to parse received config");
                server.config = parsed_config.remove(0);
                info!("Updating server config: {:#?}", server.config);
                // Republish all diagnostics
                let keys = server.watched_files.keys().cloned().collect::<Vec<_>>();
                for key in keys {
                    let watched_file = server.watched_files.get(&key).unwrap();
                    server.publish_diagnostic(
                        &key,
                        watched_file.shading_language,
                        watched_file.content.clone(),
                        None,
                    )
                }
            },
        );
    }

    fn publish_diagnostic(
        &mut self,
        uri: &Url,
        shading_language: ShadingLanguage,
        shader_source: String,
        version: Option<i32>,
    ) {
        match self.recolt_diagnostic(uri, shading_language, shader_source) {
            Ok(diagnostics) => {
                for diagnostic in diagnostics {
                    let publish_diagnostics_params = PublishDiagnosticsParams {
                        uri: diagnostic.0.clone(),
                        diagnostics: diagnostic.1,
                        version: version,
                    };
                    self.send_notification::<lsp_types::notification::PublishDiagnostics>(
                        publish_diagnostics_params,
                    );
                }
            }
            Err(err) => {
                error!("Failed to compute diagnostic for file {}: {:#?}", uri, err);
            }
        }
    }

    fn clear_diagnostic(&self, uri: &Url) {
        let publish_diagnostics_params = PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics: Vec::new(),
            version: None,
        };
        self.send_notification::<lsp_types::notification::PublishDiagnostics>(
            publish_diagnostics_params,
        );
    }
    fn send_response<N: lsp_types::request::Request>(
        &self,
        request_id: RequestId,
        params: N::Result,
    ) {
        let response = Response::new_ok::<N::Result>(request_id, params);
        self.send(response.into());
    }
    fn send_response_error(
        &self,
        request_id: RequestId,
        code: lsp_server::ErrorCode,
        message: String,
    ) {
        let response = Response::new_err(request_id, code as i32, message);
        self.send(response.into());
    }
    fn send_notification<N: lsp_types::notification::Notification>(&self, params: N::Params) {
        let not = lsp_server::Notification::new(N::METHOD.to_owned(), params);
        self.send(not.into());
    }
    fn send_request<R: lsp_types::request::Request>(
        &mut self,
        params: R::Params,
        callback: fn(&mut ServerLanguage, Value),
    ) {
        let request_id = RequestId::from(self.request_id);
        self.request_id = self.request_id + 1;
        self.request_callbacks.insert(request_id.clone(), callback);
        let req = lsp_server::Request::new(request_id, R::METHOD.to_owned(), params);
        self.send(req.into());
    }
    fn send(&self, message: Message) {
        self.connection
            .sender
            .send(message)
            .expect("Failed to send a message");
    }

    pub fn join(&mut self) -> std::io::Result<()> {
        match self.io_threads.take() {
            Some(h) => h.join(),
            None => Ok(()),
        }
    }

    pub fn get_validator(&mut self, shading_language: ShadingLanguage) -> &mut Box<dyn Validator> {
        self.validators.get_mut(&shading_language).unwrap()
    }
}

pub fn run() {
    let mut server = ServerLanguage::new();

    match server.initialize() {
        Ok(_) => info!("Server initialization successfull"),
        Err(value) => error!("Failed initalization: {:#?}", value)
    }

    match server.run() {
        Ok(_) => info!("Client disconnected"),
        Err(value) => error!("Client disconnected: {:#?}", value)
    }

    match server.join() {
        Ok(_) => info!("Server shutting down gracefully"),
        Err(value) => error!("Server failed to join threads: {:#?}", value)
    }
}
