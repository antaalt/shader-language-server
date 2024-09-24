use std::collections::HashMap;
use std::str::FromStr;

mod completion;
mod diagnostic;
mod goto;
mod hover;
mod signature;

use crate::shaders::include::Dependencies;
use crate::shaders::shader::{
    GlslSpirvVersion, GlslTargetClient, HlslShaderModel, HlslVersion, ShadingLanguage,
};
use crate::shaders::shader_error::ShaderErrorSeverity;
use crate::shaders::symbols::symbols::SymbolProvider;
#[cfg(not(target_os = "wasi"))]
use crate::shaders::validator::dxc::Dxc;
use crate::shaders::validator::glslang::Glslang;
use crate::shaders::validator::naga::Naga;
use crate::shaders::validator::validator::{ValidationParams, Validator};
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
    SignatureHelpParams, TextDocumentItem, TextDocumentSyncKind, Url, WorkDoneProgressOptions,
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
    pub autocomplete: bool,
    pub includes: Vec<String>,
    pub defines: HashMap<String, String>,
    pub validateOnType: bool,
    pub validateOnSave: bool,
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
            autocomplete: true,
            includes: Vec::new(),
            defines: HashMap::new(),
            validateOnType: true,
            validateOnSave: true,
            severity: ShaderErrorSeverity::Hint.to_string(),
            hlsl: ServerHlslConfig::default(),
            glsl: ServerGlslConfig::default(),
        }
    }
}
struct ServerFileCache {
    shading_language: ShadingLanguage,
    content: String,            // Store content on change as its not on disk.
    dependencies: Dependencies, // Store dependencies to link changes.
}

pub struct ServerLanguage {
    connection: Connection,
    io_threads: Option<IoThreads>,
    watched_files: HashMap<Url, ServerFileCache>,
    request_id: i32,
    request_callbacks: HashMap<RequestId, fn(&mut ServerLanguage, Value)>,
    pub config: ServerConfig,
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
                TextDocumentSyncKind::FULL,
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
                        "Requesting goto on file that is not watched".to_string(),
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
                    Err(()) => self.send_notification_error(format!(
                        "Received unhandled shading language: {}",
                        params.text_document.language_id
                    )),
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
                        None => self.send_notification_error(format!(
                            "Trying to save watched file that is not watched : {}",
                            params.text_document.uri
                        )),
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
                        None => self.send_notification_error(format!(
                            "Trying to change watched file that is not watched : {}",
                            params.text_document.uri
                        )),
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
            _ => warn!("Received unhandled notification: {:#?}", notification),
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
                    Some(_) => self.send_notification_error(format!(
                        "Adding a file that is already watched : {}",
                        text_document.uri
                    )),
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
            None => self.send_notification_error(format!(
                "Trying to change content of file that is not watched : {}",
                uri
            )),
        };
    }
    pub fn update_watched_file_dependencies(&mut self, uri: &Url, dependencies: Dependencies) {
        match self.watched_files.get_mut(uri) {
            Some(file) => file.dependencies = dependencies,
            None => self.send_notification_error(format!(
                "Trying to change dependencies of file that is not watched : {}",
                uri
            )),
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
            None => self.send_notification_error(format!(
                "Trying to visit file that is not watched: {}",
                uri
            )),
        };
    }
    #[allow(dead_code)]
    fn visit_watched_file_mut<F: Fn(&mut ServerFileCache)>(&mut self, uri: &Url, callback: F) {
        match self.watched_files.get_mut(uri) {
            Some(file) => callback(file),
            None => self.send_notification_error(format!(
                "Trying to visit file that is not watched: {}",
                uri
            )),
        };
    }
    fn remove_watched_file(&mut self, uri: &Url) {
        match self.watched_files.remove(&uri) {
            Some(_) => {}
            None => self.send_notification_error(format!(
                "Trying to visit file that is not watched: {}",
                uri
            )),
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
        error!("{}", message);
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
