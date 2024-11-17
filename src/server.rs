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

mod server_config;
mod server_connection;
mod server_language_data;

use crate::shaders::shader::ShadingLanguage;
use log::{debug, error, info, warn};
use lsp_types::notification::{
    DidChangeConfiguration, DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument,
    DidSaveTextDocument, Notification,
};
use lsp_types::request::{
    Completion, DocumentDiagnosticRequest, GotoDefinition, HoverRequest, Request,
    SignatureHelpRequest, WorkspaceConfiguration,
};
use lsp_types::ServerCapabilities;
use lsp_types::{
    CompletionOptionsCompletionItem, CompletionParams, CompletionResponse, ConfigurationParams,
    DidChangeConfigurationParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, DocumentDiagnosticParams,
    DocumentDiagnosticReport, DocumentDiagnosticReportKind, DocumentDiagnosticReportResult,
    FullDocumentDiagnosticReport, GotoDefinitionParams, HoverParams, HoverProviderCapability,
    RelatedFullDocumentDiagnosticReport, SignatureHelpOptions, SignatureHelpParams,
    TextDocumentSyncKind, Url, WorkDoneProgressOptions,
};

use lsp_server::{ErrorCode, Message};

use serde_json::Value;
use server_config::ServerConfig;
use server_connection::ServerConnection;
use server_language_data::{ServerFileCacheHandle, ServerLanguageData};

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
        let client_initialization_params = self.connection.initialize(server_capabilities);
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
        match self.connection.remove_callback(&response.id) {
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

                // Skip non file uri.
                if uri.scheme() != "file" {
                    self.connection.send_notification_error(format!(
                        "Trying to watch file with unsupported scheme : {}",
                        uri.scheme()
                    ));
                    return Ok(());
                }
                match ShadingLanguage::from_str(params.text_document.language_id.as_str()) {
                    Ok(shading_language) => match self.language_data.get_mut(&shading_language) {
                        Some(language_data) => {
                            match language_data.watched_files.watch_file(
                                &uri,
                                shading_language.clone(),
                                &params.text_document.text,
                                &mut language_data.symbol_provider,
                                &language_data.config,
                            ) {
                                Ok(cached_file) => {
                                    // Dont care if we replace file_language input.
                                    self.file_language
                                        .insert(uri.clone(), shading_language.clone());
                                    language_data.publish_diagnostic(
                                        &self.connection,
                                        &uri,
                                        &cached_file,
                                        Some(params.text_document.version),
                                    );
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
                                    && RefCell::borrow(&cached_file).symbol_tree.content
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
                        language_data.publish_diagnostic(connection, &uri, &cached_file, None);
                    },
                );
            }
            DidCloseTextDocument::METHOD => {
                let params: DidCloseTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let uri = clean_url(&params.text_document.uri);
                debug!("got did close text document: {:#?}", uri);
                let mut is_removed = false;
                self.visit_watched_file(
                    &uri,
                    &mut |connection: &mut ServerConnection,
                          _shading_language: ShadingLanguage,
                          language_data: &mut ServerLanguageData,
                          _cached_file: ServerFileCacheHandle| {
                        match language_data.watched_files.remove_file(&uri) {
                            Ok(was_removed) => {
                                if was_removed {
                                    language_data.clear_diagnostic(connection, &uri);
                                    is_removed = true;
                                }
                            }
                            Err(err) => connection.send_notification_error(format!("{}", err)),
                        }
                    },
                );
                if is_removed {
                    self.file_language.remove(&uri);
                }
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
                                Ok(_) => {}
                                Err(err) => connection.send_notification_error(format!("{}", err)),
                            };
                        }
                        language_data.publish_diagnostic(
                            connection,
                            &uri,
                            &cached_file,
                            Some(params.text_document.version),
                        );
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
            _ => info!("Received unhandled notification: {:#?}", notification),
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
                Some(language_data) => match language_data.watched_files.get(&uri) {
                    Some(cached_file) => {
                        visitor(
                            &mut self.connection,
                            shading_language.clone(),
                            language_data,
                            cached_file,
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
