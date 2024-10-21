use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::str::FromStr;

mod completion;
mod diagnostic;
mod goto;
mod hover;
mod signature;

use crate::shaders::include::IncludeHandler;
use crate::shaders::shader::{
    GlslSpirvVersion, GlslTargetClient, HlslShaderModel, HlslVersion, ShadingLanguage,
};
use crate::shaders::shader_error::ShaderErrorSeverity;
use crate::shaders::symbols::symbols::{ShaderSymbolList, SymbolError, SymbolProvider};
#[cfg(not(target_os = "wasi"))]
use crate::shaders::validator::dxc::Dxc;
use crate::shaders::validator::glslang::Glslang;
use crate::shaders::validator::naga::Naga;
use crate::shaders::validator::validator::{ValidationParams, Validator};
use hover::lsp_range_to_shader_range;
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
    CompletionOptionsCompletionItem, CompletionParams, CompletionResponse, ConfigurationParams,
    DidChangeConfigurationParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, DocumentDiagnosticParams,
    DocumentDiagnosticReport, DocumentDiagnosticReportResult, FullDocumentDiagnosticReport,
    GotoDefinitionParams, HoverParams, HoverProviderCapability, MessageType,
    RelatedFullDocumentDiagnosticReport, ShowMessageParams, SignatureHelpOptions,
    SignatureHelpParams, TextDocumentSyncKind, Url, WorkDoneProgressOptions,
};
use lsp_types::{InitializeParams, ServerCapabilities};

use lsp_server::{Connection, ErrorCode, IoThreads, Message, RequestId, Response};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[allow(non_snake_case)]
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ServerHlslConfig {
    pub shaderModel: HlslShaderModel,
    pub version: HlslVersion,
    pub enable16bitTypes: bool,
}
#[allow(non_snake_case)]
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ServerGlslConfig {
    pub targetClient: GlslTargetClient,
    pub spirvVersion: GlslSpirvVersion,
}

#[allow(non_snake_case)]
#[derive(Debug, Serialize, Deserialize)]
pub struct ServerConfig {
    pub includes: Vec<String>,
    pub defines: HashMap<String, String>,
    pub validate: bool,
    pub symbols: bool,
    pub severity: String,
    pub hlsl: ServerHlslConfig,
    pub glsl: ServerGlslConfig,
}

impl ServerConfig {
    fn into_validation_params(&self) -> ValidationParams {
        ValidationParams {
            includes: self.includes.clone(),
            defines: self.defines.clone(),
            hlsl_shader_model: self.hlsl.shaderModel,
            hlsl_version: self.hlsl.version,
            hlsl_enable16bit_types: self.hlsl.enable16bitTypes,
            glsl_client: self.glsl.targetClient,
            glsl_spirv: self.glsl.spirvVersion,
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            includes: Vec::new(),
            defines: HashMap::new(),
            validate: true,
            symbols: true,
            severity: ShaderErrorSeverity::Hint.to_string(),
            hlsl: ServerHlslConfig::default(),
            glsl: ServerGlslConfig::default(),
        }
    }
}

type ServerFileCacheHandle = Rc<RefCell<ServerFileCache>>;

#[derive(Debug, Clone)]
pub struct ServerFileCache {
    shading_language: ShadingLanguage,
    content: String,                // Store content on change as its not on disk.
    symbol_cache: ShaderSymbolList, // Store symbol to avoid computing them at every change.
    dependencies: HashMap<PathBuf, ServerFileCacheHandle>, // Store all dependencies of this file.
    is_open_in_editor: bool,        // Is the file a deps or is it open in editor.
}

pub struct ServerLanguage {
    connection: Connection,
    io_threads: Option<IoThreads>,
    watched_files: HashMap<Url, ServerFileCacheHandle>,
    request_id: i32,
    request_callbacks: HashMap<RequestId, fn(&mut ServerLanguage, Value)>,
    config: ServerConfig,
    validators: HashMap<ShadingLanguage, Box<dyn Validator>>,
    symbol_providers: HashMap<ShadingLanguage, SymbolProvider>,
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

        let mut symbol_providers = HashMap::new();
        symbol_providers.insert(ShadingLanguage::Glsl, SymbolProvider::glsl());
        symbol_providers.insert(ShadingLanguage::Hlsl, SymbolProvider::hlsl());
        symbol_providers.insert(ShadingLanguage::Wgsl, SymbolProvider::wgsl());
        // Run the server and wait for the two threads to end (typically by trigger LSP Exit event).
        Self {
            connection,
            io_threads: Some(io_threads),
            watched_files: HashMap::new(),
            request_id: 0,
            request_callbacks: HashMap::new(),
            config: ServerConfig::default(),
            validators: validators,
            symbol_providers: symbol_providers,
        }
    }
    pub fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
        let server_capabilities = serde_json::to_value(&ServerCapabilities {
            text_document_sync: Some(lsp_types::TextDocumentSyncCapability::Kind(
                TextDocumentSyncKind::INCREMENTAL,
            )),
            completion_provider: Some(lsp_types::CompletionOptions {
                resolve_provider: None, // For more detailed data
                completion_item: Some(CompletionOptionsCompletionItem {
                    label_details_support: Some(true),
                }),
                trigger_characters: Some(vec![".".into()]),
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
            type_definition_provider: Some(lsp_types::TypeDefinitionProviderCapability::Simple(
                false,
            )), // Disable as definition_provider is doing it.
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
                let uri = self.clean_url(&params.text_document.uri);
                match self.get_watched_file(&uri) {
                    Some(cached_file) => {
                        match self.recolt_diagnostic(&uri, Rc::clone(&cached_file)) {
                            Ok(diagnostics) => {
                                for diagnostic in diagnostics {
                                    if diagnostic.0 == uri {
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
                        format!(
                            "Requesting diagnostic on file {} that is not watched",
                            uri.path()
                        ),
                    ),
                }
            }
            GotoDefinition::METHOD => {
                let params: GotoDefinitionParams = serde_json::from_value(req.params)?;
                debug!("Received gotoDefinition request #{}: {:#?}", req.id, params);
                let uri = self.clean_url(&params.text_document_position_params.text_document.uri);
                match self.get_watched_file(&uri) {
                    Some(cached_file) => {
                        let position = params.text_document_position_params.position;
                        match self.recolt_goto(&uri, Rc::clone(&cached_file), position) {
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
                        format!("Requesting goto on file {} that is not watched", uri.path()),
                    ),
                }
            }
            Completion::METHOD => {
                let params: CompletionParams = serde_json::from_value(req.params)?;
                debug!("Received completion request #{}: {:#?}", req.id, params);
                let uri = self.clean_url(&params.text_document_position.text_document.uri);
                match self.get_watched_file(&uri) {
                    Some(cached_file) => {
                        match self.recolt_completion(
                            &uri,
                            Rc::clone(&cached_file),
                            params.text_document_position.position,
                            match params.context {
                                Some(context) => context.trigger_character,
                                None => None,
                            },
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
                        format!(
                            "Requesting diagnostic on file {} that is not watched",
                            uri.path()
                        ),
                    ),
                }
            }
            SignatureHelpRequest::METHOD => {
                let params: SignatureHelpParams = serde_json::from_value(req.params)?;
                debug!("Received completion request #{}: {:#?}", req.id, params);
                let uri = self.clean_url(&params.text_document_position_params.text_document.uri);
                match self.get_watched_file(&uri) {
                    Some(cached_file) => {
                        match self.recolt_signature(
                            &uri,
                            Rc::clone(&cached_file),
                            params.text_document_position_params.position,
                        ) {
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
                        format!(
                            "Requesting signature on file {} that is not watched",
                            uri.path()
                        ),
                    ),
                }
            }
            HoverRequest::METHOD => {
                let params: HoverParams = serde_json::from_value(req.params)?;
                debug!("Received hover request #{}: {:#?}", req.id, params);
                let uri = self.clean_url(&params.text_document_position_params.text_document.uri);
                match self.get_watched_file(&uri) {
                    Some(cached_file) => {
                        let position = params.text_document_position_params.position;
                        match self.recolt_hover(&uri, Rc::clone(&cached_file), position) {
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
                        format!(
                            "Requesting hover on file {} that is not watched",
                            uri.path()
                        ),
                    ),
                }
            }
            _ => warn!("Received unhandled request: {:#?}", req),
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
                let uri = self.clean_url(&params.text_document.uri);
                match ShadingLanguage::from_str(params.text_document.language_id.as_str()) {
                    Ok(lang) => {
                        match self.watch_file(&uri, lang, &params.text_document.text, true) {
                            Ok(_) => {}
                            Err(err) => self.send_notification_error(format!("{}", err)),
                        }
                    }
                    Err(_err) => self.send_notification_error(format!(
                        "Failed to parse language id : {}",
                        params.text_document.language_id
                    )),
                }
            }
            DidSaveTextDocument::METHOD => {
                let params: DidSaveTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let uri = self.clean_url(&params.text_document.uri);
                debug!("got did save text document: {:#?}", uri);
                // File content is updated through DidChangeTextDocument.
                match self.get_watched_file(&uri) {
                    Some(file) => {
                        let current_content = file.borrow().content.clone();
                        match self.update_watched_file_content(&uri, None, &current_content, None) {
                            Ok(_) => {}
                            Err(err) => self.send_notification_error(format!("{}", err)),
                        };
                    }
                    None => {}
                };
            }
            DidCloseTextDocument::METHOD => {
                let params: DidCloseTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let uri = self.clean_url(&params.text_document.uri);
                debug!("got did close text document: {:#?}", uri);
                self.remove_watched_file(&uri, true);
            }
            DidChangeTextDocument::METHOD => {
                let params: DidChangeTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let uri = self.clean_url(&params.text_document.uri);
                debug!("got did change text document: {:#?}", uri);
                for content in params.content_changes {
                    match self.update_watched_file_content(
                        &uri,
                        content.range,
                        &content.text,
                        Some(params.text_document.version),
                    ) {
                        Ok(_) => {}
                        Err(err) => self.send_notification_error(format!("{}", err)),
                    };
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
            _ => warn!("Received unhandled notification: {:#?}", notification),
        }
        Ok(())
    }

    fn watch_file(
        &mut self,
        uri: &Url,
        lang: ShadingLanguage,
        text: &String,
        is_open_in_editor: bool,
    ) -> Result<ServerFileCacheHandle, SymbolError> {
        let uri = self.clean_url(&uri);
        let file_path = Self::to_file_path(&uri);

        let validation_params = self.config.into_validation_params();
        // Dispatch watch_file to direct children, which will recurse all includes.
        let mut include_handler = IncludeHandler::new(&file_path, self.config.includes.clone());
        let file_dependencies = SymbolProvider::find_file_dependencies(&mut include_handler, text);
        for file_dependency in file_dependencies {
            let deps_url = Url::from_file_path(&file_dependency).unwrap();
            match self.watched_files.get(&deps_url) {
                Some(_) => {} // Already watched.
                None => {
                    let _ = self.watch_file(
                        &deps_url,
                        lang,
                        &std::fs::read_to_string(&file_dependency).unwrap(),
                        false,
                    )?;
                }
            };
        }
        self.get_symbol_provider_mut(lang)
            .create_ast(&file_path, &text)?;
        // OPTIM: Here get_all_symbols execute on whole deps. They are computed & parsed. Should be done per deps.
        let symbol_list = self.get_symbol_provider_mut(lang).get_all_symbols(
            &text,
            &file_path,
            &validation_params,
        )?;
        // Check watched file already watched
        let rc = match self.watched_files.get(&uri) {
            Some(rc) => {
                if is_open_in_editor {
                    debug!("File {} is opened in editor.", uri);
                    RefCell::borrow_mut(rc).is_open_in_editor = true;
                }
                Rc::clone(&rc)
            }
            None => {
                let rc = Rc::new(RefCell::new(ServerFileCache {
                    shading_language: lang,
                    content: text.clone(),
                    symbol_cache: if self.config.symbols {
                        symbol_list
                    } else {
                        ShaderSymbolList::default()
                    },
                    dependencies: HashMap::new(),
                    is_open_in_editor,
                }));
                let none = self.watched_files.insert(uri.clone(), Rc::clone(&rc));
                assert!(none.is_none());
                rc
            }
        };
        if is_open_in_editor {
            self.publish_diagnostic(&uri, Rc::clone(&rc), None);
        }
        debug!("Starting watching {:#?} file at {:#?}", lang, uri);
        Ok(rc)
    }
    fn update_watched_file_content(
        &mut self,
        uri: &Url,
        range: Option<lsp_types::Range>,
        partial_content: &String,
        version: Option<i32>,
    ) -> Result<(), SymbolError> {
        let now_start = std::time::Instant::now();
        let cached_file = match self.watched_files.get(uri) {
            Some(file) => Rc::clone(&file),
            None => {
                return Err(SymbolError::InternalErr(format!(
                    "Trying to change content of file that is not watched : {}",
                    uri
                )))
            }
        };
        let old_content = RefCell::borrow(&cached_file).content.clone();
        let shading_language = RefCell::borrow(&cached_file).shading_language;
        let now_update_ast = std::time::Instant::now();
        // Update abstract syntax tree
        let file_path = Self::to_file_path(&uri);
        let validation_params = self.config.into_validation_params();
        let new_content = match range {
            Some(range) => {
                let shader_range = lsp_range_to_shader_range(&range, &file_path);
                let mut new_content = old_content.clone();
                new_content.replace_range(
                    shader_range.start.to_byte_offset(&old_content)
                        ..shader_range.end.to_byte_offset(&old_content),
                    &partial_content,
                );
                self.get_symbol_provider_mut(shading_language).update_ast(
                    &file_path,
                    &old_content,
                    &new_content,
                    &shader_range,
                    &partial_content,
                )?;
                new_content
            }
            None => {
                self.get_symbol_provider_mut(shading_language)
                    .create_ast(&file_path, &partial_content)?;
                // if no range set, partial_content has whole content.
                partial_content.clone()
            }
        };
        debug!(
            "timing:update_watched_file_content:ast           {}ms",
            now_update_ast.elapsed().as_millis()
        );

        let now_get_symbol = std::time::Instant::now();
        // Cache symbols
        let symbol_list = self
            .get_symbol_provider_mut(shading_language)
            .get_all_symbols(&new_content, &file_path, &validation_params)?;
        {
            let mut cached_file_mut = RefCell::borrow_mut(&cached_file);
            cached_file_mut.symbol_cache = if self.config.symbols {
                symbol_list
            } else {
                ShaderSymbolList::default()
            };
            cached_file_mut.content = new_content;
        }
        debug!(
            "timing:update_watched_file_content:get_all_symb  {}ms",
            now_get_symbol.elapsed().as_millis()
        );

        let now_diag = std::time::Instant::now();
        // Execute diagnostic
        self.publish_diagnostic(&uri, Rc::clone(&cached_file), version);
        debug!(
            "timing:update_watched_file_content:diagnostics   {}ms",
            now_diag.elapsed().as_millis()
        );

        let now_deps = std::time::Instant::now();
        // Update files depending on this file.
        let symbol_provider = self.symbol_providers.get(&shading_language).unwrap();
        for (uri, watched_file) in &mut self.watched_files {
            let watched_file_path = Self::to_file_path(&uri);
            if file_path == watched_file_path {
                continue; // Skip same file.
            }
            let mut watched_file_mut = RefCell::borrow_mut(&watched_file);
            for dependency in &watched_file_mut.dependencies {
                if *dependency.0 == file_path {
                    // Dont need to update AST as its file dependent, only cache symbols again.
                    match symbol_provider.get_all_symbols(
                        &watched_file_mut.content,
                        &file_path,
                        &validation_params,
                    ) {
                        Ok(symbol_list) => {
                            watched_file_mut.symbol_cache = if self.config.symbols {
                                symbol_list
                            } else {
                                ShaderSymbolList::default()
                            }
                        }
                        Err(_) => warn!(
                            "Failed to get_all_symbols for deps {}",
                            dependency.0.display()
                        ), // skip
                    };
                    break;
                }
            }
        }
        debug!(
            "timing:update_watched_file_content:dependencies  {}ms",
            now_deps.elapsed().as_millis()
        );
        debug!(
            "timing:update_watched_file_content:              {}ms",
            now_start.elapsed().as_millis()
        );
        Ok(())
    }
    fn get_watched_file(&self, uri: &Url) -> Option<ServerFileCacheHandle> {
        assert!(*uri == self.clean_url(&uri));
        match self.watched_files.get(uri) {
            Some(cached_file) => Some(Rc::clone(&cached_file)),
            None => None,
        }
    }
    fn remove_watched_file(&mut self, uri: &Url, is_closed_in_editor: bool) {
        match self.watched_files.get(&uri) {
            Some(rc) => {
                let is_open_in_editor = if is_closed_in_editor {
                    let mut rc = RefCell::borrow_mut(rc);
                    rc.is_open_in_editor = false;
                    false
                } else {
                    RefCell::borrow(rc).is_open_in_editor
                };
                // Remove AST
                let file_path = Self::to_file_path(&uri);
                let lang = RefCell::borrow(rc).shading_language;
                let count = Rc::strong_count(&rc) == 1;

                // Check if deps depends on it && if its open for edit
                if count && !is_open_in_editor {
                    match self.get_symbol_provider_mut(lang).remove_ast(&file_path) {
                        Ok(_) => {}
                        Err(err) => self.send_notification_error(format!(
                            "Error creating AST for file {}: {:#?}",
                            file_path.display(),
                            err
                        )),
                    }
                    self.clear_diagnostic(&uri);
                    let removed_file = self.watched_files.remove(&uri).unwrap();
                    for dependency in &RefCell::borrow(&removed_file).dependencies {
                        self.remove_watched_file(
                            &Url::from_file_path(dependency.0).unwrap(),
                            false,
                        );
                    }
                }
            }
            None => self.send_notification_error(format!(
                "Trying to remove file {} that is not watched",
                uri.path()
            )),
        };
    }
    fn clean_url(&self, url: &Url) -> Url {
        // Workaround issue with url encoded as &3a that break key comparison.
        // Clean it by converting back & forth.
        Url::from_file_path(
            url.to_file_path()
                .expect(format!("Failed to convert {} to a valid path.", url).as_str()),
        )
        .unwrap()
    }
    fn to_file_path(cleaned_url: &Url) -> PathBuf {
        // Workaround issue with url encoded as &3a that break key comparison.
        // Clean it by converting back & forth.
        cleaned_url.to_file_path().unwrap()
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
                    match server.watched_files.get(&key) {
                        Some(cached_file) => {
                            let content = RefCell::borrow(&cached_file).content.clone();
                            // Clear diags
                            server.clear_diagnostic(&key);
                            // Update symbols & republish diags.
                            match server.update_watched_file_content(&key, None, &content, None) {
                                Ok(_) => {}
                                Err(err) => server.send_notification_error(format!("{}", err)),
                            };
                        }
                        None => {}
                    }
                }
            },
        );
    }

    pub fn send_response<N: lsp_types::request::Request>(
        &self,
        request_id: RequestId,
        params: N::Result,
    ) {
        let response = Response::new_ok::<N::Result>(request_id, params);
        self.send(response.into());
    }
    pub fn send_response_error(
        &self,
        request_id: RequestId,
        code: lsp_server::ErrorCode,
        message: String,
    ) {
        let response = Response::new_err(request_id, code as i32, message);
        self.send(response.into());
    }
    pub fn send_notification<N: lsp_types::notification::Notification>(&self, params: N::Params) {
        let not = lsp_server::Notification::new(N::METHOD.to_owned(), params);
        self.send(not.into());
    }
    pub fn send_notification_error(&self, message: String) {
        error!("NOTIFICATION: {}", message);
        self.send_notification::<lsp_types::notification::ShowMessage>(ShowMessageParams {
            typ: MessageType::ERROR,
            message: message,
        })
    }
    pub fn send_request<R: lsp_types::request::Request>(
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

    fn join(&mut self) -> std::io::Result<()> {
        match self.io_threads.take() {
            Some(h) => h.join(),
            None => Ok(()),
        }
    }

    pub fn get_validator(&mut self, shading_language: ShadingLanguage) -> &mut Box<dyn Validator> {
        self.validators.get_mut(&shading_language).unwrap()
    }

    pub fn get_symbol_provider_mut(
        &mut self,
        shading_language: ShadingLanguage,
    ) -> &mut SymbolProvider {
        self.symbol_providers.get_mut(&shading_language).unwrap()
    }

    pub fn get_symbol_provider(&self, shading_language: ShadingLanguage) -> &SymbolProvider {
        self.symbol_providers.get(&shading_language).unwrap()
    }
}

pub fn run() {
    let mut server = ServerLanguage::new();

    match server.initialize() {
        Ok(_) => info!("Server initialization successfull"),
        Err(value) => error!("Failed initalization: {:#?}", value),
    }

    match server.run() {
        Ok(_) => info!("Client disconnected"),
        Err(value) => error!("Client disconnected: {:#?}", value),
    }

    match server.join() {
        Ok(_) => info!("Server shutting down gracefully"),
        Err(value) => error!("Server failed to join threads: {:#?}", value),
    }
}
