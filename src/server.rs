use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
    DocumentDiagnosticReport, DocumentDiagnosticReportKind, DocumentDiagnosticReportResult,
    FullDocumentDiagnosticReport, GotoDefinitionParams, HoverParams, HoverProviderCapability,
    MessageType, RelatedFullDocumentDiagnosticReport, ShowMessageParams, SignatureHelpOptions,
    SignatureHelpParams, TextDocumentSyncKind, Url, WorkDoneProgressOptions,
};
use lsp_types::{InitializeParams, ServerCapabilities};

use lsp_server::{Connection, ErrorCode, IoThreads, Message, RequestId, Response};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[allow(non_snake_case)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerHlslConfig {
    pub shaderModel: HlslShaderModel,
    pub version: HlslVersion,
    pub enable16bitTypes: bool,
}
#[allow(non_snake_case)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerGlslConfig {
    pub targetClient: GlslTargetClient,
    pub spirvVersion: GlslSpirvVersion,
}

#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    is_main_file: bool,             // Is the file a deps or is it open in editor.
}
pub struct ServerLanguageFileCache {
    files: HashMap<Url, ServerFileCacheHandle>,
}
pub struct ServerLanguageData {
    watched_files: ServerLanguageFileCache,
    validator: Box<dyn Validator>,
    symbol_provider: SymbolProvider,
    config: ServerConfig,
}

pub struct ServerConnection {
    connection: Connection,
    io_threads: Option<IoThreads>,
    request_id: i32,
    request_callbacks: HashMap<RequestId, fn(&mut ServerLanguage, Value)>,
}

pub struct ServerLanguage {
    connection: ServerConnection,
    // Cache
    file_language: HashMap<Url, ShadingLanguage>,
    language_data: HashMap<ShadingLanguage, ServerLanguageData>,
}

// Handle non-utf8 characters
pub fn read_string_lossy(file_path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    match std::fs::read_to_string(file_path) {
        Ok(content) => Ok(content),
        Err(err) => match err.kind() {
            std::io::ErrorKind::InvalidData => {
                // Load non utf8 file as lossy string.
                log::warn!(
                    "Non UTF8 characters detected in file {}. Loaded as lossy string.",
                    file_path.display()
                );
                let mut file = std::fs::File::open(file_path).unwrap();
                let mut buf = vec![];
                file.read_to_end(&mut buf).unwrap();
                Ok(String::from_utf8_lossy(&buf).into())
            }
            _ => Err(err),
        },
    }
}
fn clean_url(url: &Url) -> Url {
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

impl ServerLanguageFileCache {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }
}

impl ServerLanguageData {
    pub fn glsl() -> Self {
        Self {
            watched_files: ServerLanguageFileCache::new(),
            validator: Box::new(Glslang::glsl()),
            symbol_provider: SymbolProvider::glsl(),
            config: ServerConfig::default(),
        }
    }
    pub fn hlsl() -> Self {
        Self {
            watched_files: ServerLanguageFileCache::new(),
            #[cfg(target_os = "wasi")]
            validator: Box::new(Glslang::hlsl()),
            #[cfg(not(target_os = "wasi"))]
            validator: Box::new(Dxc::new().unwrap()),
            symbol_provider: SymbolProvider::hlsl(),
            config: ServerConfig::default(),
        }
    }
    pub fn wgsl() -> Self {
        Self {
            watched_files: ServerLanguageFileCache::new(),
            validator: Box::new(Naga::new()),
            symbol_provider: SymbolProvider::wgsl(),
            config: ServerConfig::default(),
        }
    }
}
impl ServerConnection {
    pub fn new() -> Self {
        // Create the transport. Includes the stdio (stdin and stdout) versions but this could
        // also be implemented to use sockets or HTTP.
        let (connection, io_threads) = Connection::stdio();
        Self {
            connection,
            io_threads: Some(io_threads),
            request_id: 0,
            request_callbacks: HashMap::new(),
        }
    }
}
impl ServerLanguage {
    pub fn new() -> Self {
        // Run the server and wait for the two threads to end (typically by trigger LSP Exit event).
        Self {
            connection: ServerConnection::new(),
            file_language: HashMap::new(),
            language_data: HashMap::from([
                (ShadingLanguage::Glsl, ServerLanguageData::glsl()),
                (ShadingLanguage::Hlsl, ServerLanguageData::hlsl()),
                (ShadingLanguage::Wgsl, ServerLanguageData::wgsl()),
            ]),
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
        let initialization_params = match self.connection.connection.initialize(server_capabilities)
        {
            Ok(it) => it,
            Err(e) => {
                if e.channel_is_disconnected() {
                    self.connection.io_threads.take().unwrap().join()?;
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
            let msg_err = self.connection.connection.receiver.recv();
            match msg_err {
                Ok(msg) => match msg {
                    Message::Request(req) => {
                        if self.connection.connection.handle_shutdown(&req)? {
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
                let uri = clean_url(&params.text_document.uri);
                self.visit_watched_file(
                    &uri,
                    &mut |connection: &mut ServerConnection,
                          _shading_language: ShadingLanguage,
                          language_data: &mut ServerLanguageData,
                          cached_file: ServerFileCacheHandle| {
                        match language_data.recolt_diagnostic(&uri, &cached_file) {
                            Ok(mut diagnostics) => {
                                let main_diagnostic = match diagnostics.remove(&uri) {
                                    Some(diag) => diag,
                                    None => vec![],
                                };
                                connection.send_response::<DocumentDiagnosticRequest>(
                                    req.id.clone(),
                                    DocumentDiagnosticReportResult::Report(
                                        DocumentDiagnosticReport::Full(
                                            RelatedFullDocumentDiagnosticReport {
                                                related_documents: Some(
                                                    diagnostics
                                                        .into_iter()
                                                        .map(|diagnostic| {
                                                            (
                                                                diagnostic.0,
                                                                DocumentDiagnosticReportKind::Full(
                                                                    FullDocumentDiagnosticReport {
                                                                        result_id: Some(
                                                                            req.id.to_string(),
                                                                        ),
                                                                        items: diagnostic.1,
                                                                    },
                                                                ),
                                                            )
                                                        })
                                                        .collect(),
                                                ),
                                                full_document_diagnostic_report:
                                                    FullDocumentDiagnosticReport {
                                                        result_id: Some(req.id.to_string()),
                                                        items: main_diagnostic,
                                                    },
                                            },
                                        ),
                                    ),
                                )
                            }
                            // Send empty report.
                            Err(error) => connection.send_response_error(
                                req.id.clone(),
                                lsp_server::ErrorCode::InternalError,
                                error.to_string(),
                            ),
                        };
                    },
                );
            }
            GotoDefinition::METHOD => {
                let params: GotoDefinitionParams = serde_json::from_value(req.params)?;
                debug!("Received gotoDefinition request #{}: {:#?}", req.id, params);
                let uri = clean_url(&params.text_document_position_params.text_document.uri);
                self.visit_watched_file(
                    &uri,
                    &mut |connection: &mut ServerConnection,
                          _shading_language: ShadingLanguage,
                          language_data: &mut ServerLanguageData,
                          cached_file: ServerFileCacheHandle| {
                        let position = params.text_document_position_params.position;
                        match language_data.recolt_goto(&uri, Rc::clone(&cached_file), position) {
                            Ok(value) => {
                                connection.send_response::<GotoDefinition>(req.id.clone(), value)
                            }
                            Err(err) => connection.send_response_error(
                                req.id.clone(),
                                ErrorCode::InvalidParams,
                                format!("Failed to recolt signature : {:#?}", err),
                            ),
                        }
                    },
                );
            }
            Completion::METHOD => {
                let params: CompletionParams = serde_json::from_value(req.params)?;
                debug!("Received completion request #{}: {:#?}", req.id, params);
                let uri = clean_url(&params.text_document_position.text_document.uri);
                self.visit_watched_file(
                    &uri,
                    &mut |connection: &mut ServerConnection,
                          _shading_language: ShadingLanguage,
                          language_data: &mut ServerLanguageData,
                          cached_file: ServerFileCacheHandle| {
                        match language_data.recolt_completion(
                            &uri,
                            Rc::clone(&cached_file),
                            params.text_document_position.position,
                            match &params.context {
                                Some(context) => context.trigger_character.clone(),
                                None => None,
                            },
                        ) {
                            Ok(value) => connection.send_response::<Completion>(
                                req.id.clone(),
                                Some(CompletionResponse::Array(value)),
                            ),
                            Err(error) => connection.send_response_error(
                                req.id.clone(),
                                lsp_server::ErrorCode::InternalError,
                                error.to_string(),
                            ),
                        }
                    },
                );
            }
            SignatureHelpRequest::METHOD => {
                let params: SignatureHelpParams = serde_json::from_value(req.params)?;
                debug!("Received completion request #{}: {:#?}", req.id, params);
                let uri = clean_url(&params.text_document_position_params.text_document.uri);
                self.visit_watched_file(
                    &uri,
                    &mut |connection: &mut ServerConnection,
                          _shading_language: ShadingLanguage,
                          language_data: &mut ServerLanguageData,
                          cached_file: ServerFileCacheHandle| {
                        match language_data.recolt_signature(
                            &uri,
                            Rc::clone(&cached_file),
                            params.text_document_position_params.position,
                        ) {
                            Ok(value) => connection
                                .send_response::<SignatureHelpRequest>(req.id.clone(), value),
                            Err(err) => connection.send_response_error(
                                req.id.clone(),
                                ErrorCode::InvalidParams,
                                format!("Failed to recolt signature : {:#?}", err),
                            ),
                        }
                    },
                );
            }
            HoverRequest::METHOD => {
                let params: HoverParams = serde_json::from_value(req.params)?;
                debug!("Received hover request #{}: {:#?}", req.id, params);
                let uri = clean_url(&params.text_document_position_params.text_document.uri);
                self.visit_watched_file(
                    &uri,
                    &mut |connection: &mut ServerConnection,
                          _shading_language: ShadingLanguage,
                          language_data: &mut ServerLanguageData,
                          cached_file: ServerFileCacheHandle| {
                        let position = params.text_document_position_params.position;
                        match language_data.recolt_hover(&uri, Rc::clone(&cached_file), position) {
                            Ok(value) => {
                                connection.send_response::<HoverRequest>(req.id.clone(), value)
                            }
                            Err(err) => connection.send_response_error(
                                req.id.clone(),
                                ErrorCode::InvalidParams,
                                format!("Failed to recolt signature : {:#?}", err),
                            ),
                        }
                    },
                );
            }
            _ => warn!("Received unhandled request: {:#?}", req),
        }
        Ok(())
    }
    fn on_response(&mut self, response: lsp_server::Response) -> Result<(), serde_json::Error> {
        match self.connection.request_callbacks.remove(&response.id) {
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
                let uri = clean_url(&params.text_document.uri);
                match ShadingLanguage::from_str(params.text_document.language_id.as_str()) {
                    Ok(shading_language) => match self.file_language.get(&uri) {
                        // Check if file exist already in cache (as deps)
                        Some(shading_language) => {
                            match self.language_data.get_mut(shading_language) {
                                Some(language_data) => {
                                    match language_data.watched_files.get_watched_file(&uri) {
                                        Some(cached_file) => {
                                            // Exist as deps, mark as main file.
                                            RefCell::borrow_mut(&cached_file).is_main_file = true;
                                        }
                                        None => self.connection.send_notification_error(format!(
                                    "Trying to change content of file that is not watched : {}",
                                    uri
                                )),
                                    }
                                }
                                None => self.connection.send_notification_error(format!(
                                    "Trying to get language data with invalid language : {}",
                                    shading_language.to_string()
                                )),
                            }
                        }
                        None => match self.language_data.get_mut(&shading_language) {
                            Some(language_data) => {
                                match language_data.watched_files.watch_file(
                                    &uri,
                                    shading_language,
                                    &params.text_document.text,
                                    &mut language_data.symbol_provider,
                                    &language_data.config,
                                    true,
                                ) {
                                    Ok(_) => {
                                        self.file_language.insert(uri, shading_language);
                                    }
                                    Err(_) => self.connection.send_notification_error(format!(
                                        "Failed to watch file {}",
                                        uri.to_string()
                                    )),
                                }
                            }
                            None => self.connection.send_notification_error(format!(
                                "Trying to get language data with invalid language : {}",
                                shading_language.to_string()
                            )),
                        },
                    },
                    Err(_) => self.connection.send_notification_error(format!(
                        "Failed to parse language id : {}",
                        params.text_document.language_id
                    )),
                }
            }
            DidSaveTextDocument::METHOD => {
                let params: DidSaveTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let uri = clean_url(&params.text_document.uri);
                debug!("got did save text document: {:#?}", uri);
                // File content is updated through DidChangeTextDocument.
                self.visit_watched_file(
                    &uri,
                    &mut |connection: &mut ServerConnection,
                          _shading_language: ShadingLanguage,
                          language_data: &mut ServerLanguageData,
                          cached_file: ServerFileCacheHandle| {
                        assert!(
                            params.text.is_none()
                                || (params.text.is_some()
                                    && RefCell::borrow(&cached_file).content
                                        == *params.text.as_ref().unwrap())
                        );
                        match RefCell::borrow_mut(&cached_file).update(
                            &uri,
                            &mut language_data.symbol_provider,
                            &language_data.config,
                            None,
                            None,
                        ) {
                            Ok(_) => {}
                            Err(err) => connection.send_notification_error(format!("{}", err)),
                        };
                    },
                );
            }
            DidCloseTextDocument::METHOD => {
                let params: DidCloseTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let uri = clean_url(&params.text_document.uri);
                debug!("got did close text document: {:#?}", uri);
                self.visit_watched_file(
                    &uri,
                    &mut |connection: &mut ServerConnection,
                          _shading_language: ShadingLanguage,
                          language_data: &mut ServerLanguageData,
                          _cached_file: ServerFileCacheHandle| {
                        match language_data.watched_files.remove_watched_file(
                            &uri,
                            &mut language_data.symbol_provider,
                            &language_data.config,
                            true,
                        ) {
                            Ok(_) => {
                                language_data.clear_diagnostic(connection, &uri);
                            }
                            Err(err) => connection.send_notification_error(format!("{}", err)),
                        }
                    },
                );
            }
            DidChangeTextDocument::METHOD => {
                let params: DidChangeTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let uri = clean_url(&params.text_document.uri);
                debug!("got did change text document: {:#?}", uri);
                self.visit_watched_file(
                    &uri,
                    &mut |connection: &mut ServerConnection,
                          _shading_language: ShadingLanguage,
                          language_data: &mut ServerLanguageData,
                          cached_file: ServerFileCacheHandle| {
                        for content in &params.content_changes {
                            match RefCell::borrow_mut(&cached_file).update(
                                &uri,
                                &mut language_data.symbol_provider,
                                &language_data.config,
                                content.range,
                                Some(&content.text),
                            ) {
                                Ok(_) => {
                                    language_data.publish_diagnostic(
                                        connection,
                                        &uri,
                                        &cached_file,
                                        Some(params.text_document.version),
                                    );
                                }
                                Err(err) => connection.send_notification_error(format!("{}", err)),
                            };
                        }
                    },
                );
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
    fn visit_watched_file(
        &mut self,
        uri: &Url,
        visitor: &mut dyn FnMut(
            &mut ServerConnection,
            ShadingLanguage,
            &mut ServerLanguageData,
            ServerFileCacheHandle,
        ),
    ) {
        match self.file_language.get(&uri) {
            Some(shading_language) => match self.language_data.get_mut(shading_language) {
                Some(language_data) => match language_data.watched_files.get_watched_file(&uri) {
                    Some(rc) => {
                        visitor(
                            &mut self.connection,
                            shading_language.clone(),
                            language_data,
                            rc,
                        );
                    }
                    None => self.connection.send_notification_error(format!(
                        "Trying to change content of file that is not watched : {}",
                        uri
                    )),
                },
                None => self.connection.send_notification_error(format!(
                    "Trying to get language data with invalid language : {}",
                    shading_language.to_string()
                )),
            },
            None => self.connection.send_notification_error(format!(
                "Trying to change content of file that is not watched : {}",
                uri
            )),
        };
    }
    fn request_configuration(&mut self) {
        let config = ConfigurationParams {
            items: vec![lsp_types::ConfigurationItem {
                scope_uri: None,
                section: Some("shader-validator".to_owned()),
            }],
        };
        self.connection.send_request::<WorkspaceConfiguration>(
            config,
            |server: &mut ServerLanguage, value: Value| {
                // Sent 1 item, received 1 in an array
                let mut parsed_config: Vec<ServerConfig> =
                    serde_json::from_value(value).expect("Failed to parse received config");
                let config = parsed_config.remove(0);
                info!("Updating server config: {:#?}", config);
                for (_language, language_data) in &mut server.language_data {
                    language_data.config = config.clone();
                    // Republish all diagnostics
                    for (url, cached_file) in &language_data.watched_files.files {
                        // Clear diags
                        language_data.clear_diagnostic(&server.connection, &url);
                        // Update symbols & republish diags.
                        match RefCell::borrow_mut(&cached_file).update(
                            &url,
                            &mut language_data.symbol_provider,
                            &language_data.config,
                            None,
                            None,
                        ) {
                            Ok(_) => {}
                            Err(err) => server
                                .connection
                                .send_notification_error(format!("{}", err)),
                        };
                    }
                }
            },
        );
    }
}
impl ServerConnection {
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
}
impl ServerFileCache {
    pub fn update(
        &mut self,
        uri: &Url,
        symbol_provider: &mut SymbolProvider,
        config: &ServerConfig,
        range: Option<lsp_types::Range>,
        partial_content: Option<&String>,
    ) -> Result<(), SymbolError> {
        let now_start = std::time::Instant::now();
        let old_content = self.content.clone();
        let now_update_ast = std::time::Instant::now();
        // Update abstract syntax tree
        let file_path = to_file_path(&uri);
        let validation_params = config.into_validation_params();
        let new_content = if let (Some(range), Some(partial_content)) = (range, partial_content) {
            let shader_range = lsp_range_to_shader_range(&range, &file_path);
            let mut new_content = old_content.clone();
            new_content.replace_range(
                shader_range.start.to_byte_offset(&old_content)
                    ..shader_range.end.to_byte_offset(&old_content),
                &partial_content,
            );
            symbol_provider.update_ast(
                &file_path,
                &old_content,
                &new_content,
                &shader_range,
                &partial_content,
            )?;
            new_content
        } else if let Some(whole_content) = partial_content {
            symbol_provider.create_ast(&file_path, &whole_content)?;
            // if no range set, partial_content has whole content.
            whole_content.clone()
        } else {
            // Copy current content.
            self.content.clone()
        };
        debug!(
            "timing:update_watched_file_content:ast           {}ms",
            now_update_ast.elapsed().as_millis()
        );

        let now_get_symbol = std::time::Instant::now();
        // Cache symbols
        let symbol_list =
            symbol_provider.get_all_symbols(&new_content, &file_path, &validation_params)?;
        {
            self.symbol_cache = if config.symbols {
                symbol_list
            } else {
                ShaderSymbolList::default()
            };
            self.content = new_content;
        }
        debug!(
            "timing:update_watched_file_content:get_all_symb  {}ms",
            now_get_symbol.elapsed().as_millis()
        );
        debug!(
            "timing:update_watched_file_content:              {}ms",
            now_start.elapsed().as_millis()
        );
        Ok(())
    }
}
impl ServerLanguageFileCache {
    fn watch_file(
        &mut self,
        uri: &Url,
        lang: ShadingLanguage,
        text: &String,
        symbol_provider: &mut SymbolProvider,
        config: &ServerConfig,
        is_main_file: bool,
    ) -> Result<ServerFileCacheHandle, SymbolError> {
        let uri = clean_url(&uri);
        let file_path = to_file_path(&uri);

        // Check watched file already watched
        // TODO: instead we can pass file as optional param.
        let rc = match self.files.get(&uri) {
            Some(rc) => {
                if is_main_file {
                    debug!("File {} is opened in editor.", uri);
                    let mut rc_mut = RefCell::borrow_mut(rc);
                    rc_mut.is_main_file = true;
                    rc_mut.content = text.clone();
                    assert!(rc_mut.shading_language == lang);
                }
                Rc::clone(&rc)
            }
            None => {
                let rc = Rc::new(RefCell::new(ServerFileCache {
                    shading_language: lang,
                    content: text.clone(),
                    symbol_cache: if config.symbols {
                        let validation_params = config.into_validation_params();
                        symbol_provider.create_ast(&file_path, &text)?;
                        let symbol_list = symbol_provider.get_all_symbols(
                            &text,
                            &file_path,
                            &validation_params,
                        )?;
                        symbol_list
                    } else {
                        ShaderSymbolList::default()
                    },
                    dependencies: HashMap::new(), // Need to be inserted into watched_file before computing to avoid stack overflow.
                    is_main_file,
                }));
                let none = self.files.insert(uri.clone(), Rc::clone(&rc));
                assert!(none.is_none());
                rc
            }
        };

        // Dispatch watch_file to direct children, which will recurse all includes.
        let mut include_handler = IncludeHandler::new(&file_path, config.includes.clone());
        let file_dependencies = SymbolProvider::find_file_dependencies(&mut include_handler, text);
        let mut dependencies = HashMap::new();
        for file_dependency in file_dependencies {
            let deps_url = Url::from_file_path(&file_dependency).unwrap();
            match self.files.get(&deps_url) {
                Some(rc) => {
                    debug!("Skipping deps {}", file_dependency.display());
                    dependencies.insert(file_dependency, Rc::clone(&rc));
                } // Already watched.
                None => {
                    debug!("Loading deps {}", file_dependency.display());
                    let deps = self.watch_file(
                        &deps_url,
                        lang,
                        &read_string_lossy(&file_dependency).unwrap(),
                        symbol_provider,
                        config,
                        false,
                    )?;
                    dependencies.insert(file_dependency, deps);
                }
            };
        }
        RefCell::borrow_mut(&rc).dependencies = dependencies;
        debug!(
            "Starting watching {:#?} file at {} (is deps: {})",
            lang,
            file_path.display(),
            !is_main_file
        );
        Ok(rc)
    }
    /*fn update_watched_file_content(
        &mut self,
        uri: &Url,
        cached_file: ServerFileCacheHandle,
        symbol_provider: &mut SymbolProvider,
        config: &ServerConfig,
        range: Option<lsp_types::Range>,
        partial_content: Option<&String>,
        version: Option<i32>,
    ) -> Result<(), SymbolError> {
        let now_start = std::time::Instant::now();
        let old_content = RefCell::borrow(&cached_file).content.clone();
        let shading_language = RefCell::borrow(&cached_file).shading_language;
        let now_update_ast = std::time::Instant::now();
        // Update abstract syntax tree
        let file_path = to_file_path(&uri);
        let validation_params = config.into_validation_params();
        let new_content = if let (Some(range), Some(partial_content)) = (range, partial_content) {
            let shader_range = lsp_range_to_shader_range(&range, &file_path);
            let mut new_content = old_content.clone();
            new_content.replace_range(
                shader_range.start.to_byte_offset(&old_content)
                    ..shader_range.end.to_byte_offset(&old_content),
                &partial_content,
            );
            symbol_provider.update_ast(
                &file_path,
                &old_content,
                &new_content,
                &shader_range,
                &partial_content,
            )?;
            new_content
        } else if let Some(whole_content) = partial_content {
            symbol_provider.create_ast(&file_path, &whole_content)?;
            // if no range set, partial_content has whole content.
            whole_content.clone()
        } else {
            // Copy current content.
            RefCell::borrow(&cached_file).content.clone()
        };
        debug!(
            "timing:update_watched_file_content:ast           {}ms",
            now_update_ast.elapsed().as_millis()
        );

        let now_get_symbol = std::time::Instant::now();
        // Cache symbols
        let symbol_list = symbol_provider.get_all_symbols(&new_content, &file_path, &validation_params)?;
        {
            let mut cached_file_mut = RefCell::borrow_mut(&cached_file);
            cached_file_mut.symbol_cache = if config.symbols {
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
        // TODO: move upper
        /*if RefCell::borrow(&cached_file).is_main_file {
            self.publish_diagnostic(&uri, Rc::clone(&cached_file), version);
        }*/
        debug!(
            "timing:update_watched_file_content:diagnostics   {}ms",
            now_diag.elapsed().as_millis()
        );
        debug!(
            "timing:update_watched_file_content:              {}ms",
            now_start.elapsed().as_millis()
        );
        Ok(())
    }*/
    fn get_watched_file(&self, uri: &Url) -> Option<ServerFileCacheHandle> {
        assert!(*uri == clean_url(&uri));
        match self.files.get(uri) {
            Some(cached_file) => Some(Rc::clone(&cached_file)),
            None => None,
        }
    }
    fn remove_watched_file(
        &mut self,
        uri: &Url,
        symbol_provider: &mut SymbolProvider,
        _config: &ServerConfig,
        is_main_file: bool,
    ) -> Result<(), SymbolError> {
        fn list_all_dependencies_count(
            file_cache: &ServerFileCacheHandle,
        ) -> HashMap<PathBuf, usize> {
            let list = HashMap::new();
            for dependency in &RefCell::borrow(file_cache).dependencies {
                let mut list = HashMap::new();
                let deps = list_all_dependencies_count(&dependency.1);
                for dep in deps {
                    match list.get_mut(&dep.0) {
                        Some(count) => {
                            *count = *count + 1;
                        }
                        None => {
                            list.insert(dep.0, 1);
                        }
                    }
                }
            }
            list
        }
        // Look if its used by some deps before removing.
        match self.files.get(&uri) {
            Some(rc) => {
                let _is_main_file = if is_main_file {
                    let mut rc = RefCell::borrow_mut(rc);
                    rc.is_main_file = false;
                    false
                } else {
                    RefCell::borrow(rc).is_main_file
                };
                let file_path = to_file_path(&uri);
                let lang = RefCell::borrow(rc).shading_language;

                debug!(
                    "Removing watched file {} with ref count {}",
                    file_path.display(),
                    Rc::strong_count(rc)
                );

                // Collect all dangling deps
                let dependencies_count = list_all_dependencies_count(rc);

                // Check if file is ref in its own deps (might happen).
                let is_last_ref = match dependencies_count.get(&file_path) {
                    Some(count) => *count + 1 == Rc::strong_count(rc),
                    None => true,
                };
                if is_last_ref {
                    self.files.remove(&uri);

                    // Remove every dangling deps
                    for (dependency_path, dependency_count) in dependencies_count {
                        let url = Url::from_file_path(&dependency_path).unwrap();
                        match self.files.get(&url) {
                            Some(dependency_file) => {
                                let ref_count = Rc::strong_count(dependency_file);
                                let is_open_in_editor =
                                    RefCell::borrow(&dependency_file).is_main_file;
                                let is_dangling =
                                    ref_count == dependency_count + 1 && !is_open_in_editor;
                                if is_dangling {
                                    match symbol_provider.remove_ast(&dependency_path) {
                                        Ok(_) => {}
                                        Err(err) => {
                                            return Err(SymbolError::InternalErr(format!(
                                                "Error removing AST for file {}: {:#?}",
                                                dependency_path.display(),
                                                err
                                            )))
                                        }
                                    }
                                    self.files.remove(&url).unwrap();
                                    debug!(
                                        "Removed dangling {:#?} file at {}",
                                        lang,
                                        dependency_path.display()
                                    );
                                }
                            }
                            None => {
                                panic!("Could not find watched file {}", dependency_path.display())
                            }
                        }
                    }
                }
                Ok(())
            }
            None => Err(SymbolError::InternalErr(format!(
                "Trying to remove file {} that is not watched",
                uri.path()
            ))),
        }
    }
}

impl ServerLanguageData {
    fn get_all_symbols(&self, cached_file: ServerFileCacheHandle) -> ShaderSymbolList {
        let cached_file = RefCell::borrow(&cached_file);
        // Add current symbols
        let mut symbol_cache = cached_file.symbol_cache.clone();
        // Add intrinsics symbols
        symbol_cache.append(self.symbol_provider.get_intrinsics_symbol().clone());
        // Add deps symbols
        for (_, deps_cached_file) in &cached_file.dependencies {
            let deps_cached_file = RefCell::borrow(&deps_cached_file);
            symbol_cache.append(deps_cached_file.symbol_cache.clone());
        }
        symbol_cache
    }

    /*pub fn get_validator(&mut self, shading_language: ShadingLanguage) -> &mut Box<dyn Validator> {
        self.validator.get_mut(&shading_language).unwrap()
    }

    pub fn get_symbol_provider_mut(
        &mut self,
        shading_language: ShadingLanguage,
    ) -> &mut SymbolProvider {
        self.symbol_provider.get_mut(&shading_language).unwrap()
    }

    pub fn get_symbol_provider(&self, shading_language: ShadingLanguage) -> &SymbolProvider {
        self.symbol_provider.get(&shading_language).unwrap()
    }*/
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

    match server.connection.join() {
        Ok(_) => info!("Server shutting down gracefully"),
        Err(value) => error!("Server failed to join threads: {:#?}", value),
    }
}
