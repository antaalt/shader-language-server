use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;
use std::path::PathBuf;

use crate::common::ShadingLanguage;
use crate::common::{ValidationParams, Validator};
#[cfg(not(target_os = "wasi"))]
use crate::dxc::Dxc;
use crate::glslang::Glslang;
use crate::naga::Naga;
use crate::shader_error::{ShaderError, ShaderErrorList, ShaderErrorSeverity};
use log::{debug, error, warn};
use lsp_types::notification::{DidChangeConfiguration, DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument, Notification};
use lsp_types::request::{DocumentDiagnosticRequest, GotoDefinition, Request};
use lsp_types::{Diagnostic, DiagnosticOptions, DiagnosticServerCapabilities, DidChangeConfigurationParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams, DocumentDiagnosticParams, DocumentDiagnosticReport, DocumentDiagnosticReportResult, FullDocumentDiagnosticReport, GotoDefinitionParams, GotoDefinitionResponse, OneOf, PublishDiagnosticsParams, RelatedFullDocumentDiagnosticReport, TextDocumentSyncKind, Url, WorkDoneProgressOptions};
use lsp_types::{
    InitializeParams, ServerCapabilities,
};

use lsp_server::{Connection, IoThreads, Message, Response};

use serde::{Deserialize, Serialize};

#[allow(non_snake_case)]
#[derive(Debug, Serialize, Deserialize)]
struct ValidateFileParams {
    path: PathBuf,
    cwd: PathBuf,
    shadingLanguage: String,
    includes: Vec<String>,
    defines: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
enum ValidateFileError {
    ParserErr {
        filename: Option<String>,
        severity: String,
        error: String,
        line: u32,
        pos: u32,
    },
    ValidationErr {
        message: String,
    },
    UnknownError(String),
}

struct ServerLanguage {
    connection: Connection,
    io_threads: Option<IoThreads>,
}

impl ServerLanguage {
    pub fn new() -> Self {
        // Create the transport. Includes the stdio (stdin and stdout) versions but this could
        // also be implemented to use sockets or HTTP.
        let (connection, io_threads) = Connection::stdio();

        // Run the server and wait for the two threads to end (typically by trigger LSP Exit event).
        Self {
            connection,
            io_threads: Some(io_threads),
        }
    }
    pub fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
        let server_capabilities = serde_json::to_value(&ServerCapabilities {
            text_document_sync: Some(lsp_types::TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
            definition_provider: Some(OneOf::Left(true)),
            diagnostic_provider: Some(DiagnosticServerCapabilities::Options(DiagnosticOptions{
                identifier: None,
                inter_file_dependencies: false, // TODO: support multi files
                workspace_diagnostics: false,
                work_done_progress_options: WorkDoneProgressOptions { work_done_progress: None },
            })),
            ..Default::default()
        }).unwrap();
        let initialization_params = match self.connection.initialize(server_capabilities) {
            Ok(it) => it,
            Err(e) => {
                if e.channel_is_disconnected() {
                    self.io_threads.take().unwrap().join()?;
                }
                return Err(e.into());
            }
        };    
        let _params: InitializeParams = serde_json::from_value(initialization_params).unwrap();
        return Ok(());
    }
    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error + Sync + Send>>
    {
        for msg in &self.connection.receiver {
            match msg {
                Message::Request(req) => {
                    if self.connection.handle_shutdown(&req)? {
                        return Ok(());
                    }
                    self.on_request(req)?;
                }
                Message::Response(resp) => {
                    warn!("Received unhandled response: {:#?}", resp);
                }
                Message::Notification(not) => {
                    self.on_notification(not)?;
                }
            }
        }
        Ok(())
    }
    fn on_request(&self, req: lsp_server::Request) -> Result<(), serde_json::Error> {
        match req.method.as_str() {
            DocumentDiagnosticRequest::METHOD => {
                let params : DocumentDiagnosticParams = serde_json::from_value(req.params)?;
                debug!("Received document diagnostic request #{}: {:#?}", req.id, params);
                let diagnostic_result = match self.recolt_diagnostic(params.text_document.uri) {
                    Some(diagnostics) => {
                        Some(DocumentDiagnosticReportResult::Report(
                            DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport{
                                related_documents: None, // TODO: data of other files.
                                full_document_diagnostic_report: FullDocumentDiagnosticReport{
                                    result_id: Some(req.id.to_string()),
                                    items: diagnostics,
                                },
                            })
                        ))
                    } 
                    None => { None }
                };
                let result = serde_json::to_value(diagnostic_result)?;
                let resp = Response { id: req.id, result: Some(result), error: None };
                self.send(Message::Response(resp));
            },
            GotoDefinition::METHOD => {
                let params : GotoDefinitionParams = serde_json::from_value(req.params)?;
                debug!("Received gotoDefinition request #{}: {:#?}", req.id, params);
                let result = Some(GotoDefinitionResponse::Array(Vec::new()));
                let result = serde_json::to_value(&result)?;
                let resp = Response { id: req.id, result: Some(result), error: None };
                self.send(Message::Response(resp));
            }
            _ => {
                warn!("Received unhandled request: {:#?}", req);
            }
        }
        Ok(())
    }
    fn on_notification(&self, notification: lsp_server::Notification) -> Result<(), serde_json::Error> {
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let params : DidOpenTextDocumentParams = serde_json::from_value(notification.params)?;
                debug!("got did open text document: {:#?}", params.text_document.uri);
                // diagnostic request sent on opening of file ?
                //self.publish_diagnostic(params.text_document.uri);
            },
            DidSaveTextDocument::METHOD => {
                let params : DidSaveTextDocumentParams = serde_json::from_value(notification.params)?;
                debug!("got did save text document: {:#?}", params.text_document.uri);
                self.publish_diagnostic(params.text_document.uri);
            },
            DidCloseTextDocument::METHOD => {
                let params : DidCloseTextDocumentParams = serde_json::from_value(notification.params)?;
                debug!("got did close text document: {:#?}", params.text_document.uri);
            },
            DidChangeTextDocument::METHOD => {
                let params : DidChangeTextDocumentParams = serde_json::from_value(notification.params)?;
                debug!("got did change text document: {:#?}", params.text_document.uri);
                self.publish_diagnostic(params.text_document.uri);
            },
            DidChangeConfiguration::METHOD => {
                let params : DidChangeConfigurationParams = serde_json::from_value(notification.params)?;
                debug!("Received did change configuration document: {:#?}", params);
                params.settings; // TODO: parse given settings
            }
            _ => {
                warn!("Received notification: {:#?}", notification);
            }
        }
        Ok(())
    }

    fn recolt_diagnostic(&self, uri: Url) -> Option<Vec<Diagnostic>> {
        let shading_language_parsed = ShadingLanguage::from_str("wgsl");
        let shading_language = match shading_language_parsed {
            Ok(res) => res,
            Err(_) => {
                // Ignore file, its not concerned by diagnostic.
                return None;
            }
        };

        // Skip non file uri.
        match uri.scheme() {
            "file" => {}
            _ => { return None; }
        }
        let path = uri.to_file_path().expect("Failed to convert path");
        let mut validator = get_validator(shading_language);
        match validator.validate_shader(
            path.as_path(),
            Path::new("./"),
            ValidationParams::new(Vec::new(), HashMap::new()),
        ) {
            Ok(_) => { 
                // no diagnostic to publish
            }
            Err(err) => {
                let mut diagnostics = Vec::new();
                for error in err.errors {
                    match error {
                        ShaderError::ParserErr{filename: _, severity, error, line, pos} => {
                            diagnostics.push(Diagnostic {
                                range: lsp_types::Range::new(lsp_types::Position::new(line - 1, pos), lsp_types::Position::new(line - 1, pos)),
                                severity: Some(match severity {
                                    ShaderErrorSeverity::Hint => lsp_types::DiagnosticSeverity::HINT,
                                    ShaderErrorSeverity::Information => lsp_types::DiagnosticSeverity::INFORMATION,
                                    ShaderErrorSeverity::Warning => lsp_types::DiagnosticSeverity::WARNING,
                                    ShaderErrorSeverity::Error => lsp_types::DiagnosticSeverity::ERROR,
                                }),
                                message: error,
                                ..Default::default()
                            });
                        },
                        ShaderError::ValidationErr{message} => {
                            diagnostics.push(Diagnostic {
                                range: lsp_types::Range::new(lsp_types::Position::new(0, 0), lsp_types::Position::new(0, 0)),
                                severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                                message: message,
                                ..Default::default()
                            });
                        },
                        ShaderError::InternalErr(err) => {
                            diagnostics.push(Diagnostic {
                                range: lsp_types::Range::new(lsp_types::Position::new(0, 0), lsp_types::Position::new(0, 0)),
                                severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                                message: format!("InternalErr({}): {}",uri.path(), err.to_string()),
                                ..Default::default()
                            });
                        },
                        ShaderError::IoErr(err) => {
                            diagnostics.push(Diagnostic {
                                range: lsp_types::Range::new(lsp_types::Position::new(0, 0), lsp_types::Position::new(0, 0)),
                                severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                                message: format!("IoErr({}): {}",uri.path(), err.to_string()),
                                ..Default::default()
                            });
                        }
                    }
                }
                return Some(diagnostics);
            }
        };
        return None;
    }

    fn publish_diagnostic(&self, uri : Url) {
        match self.recolt_diagnostic(uri.clone()) {
            Some(diagnostics) => {
                let publish_diagnostics_params = PublishDiagnosticsParams {
                    uri,
                    diagnostics,
                    version: None,
                };
                self.send_notification::<lsp_types::notification::PublishDiagnostics>(publish_diagnostics_params);
            }
            None => {}
        }
    } 

    fn send_notification<N: lsp_types::notification::Notification>(
        &self,
        params: N::Params,
    ) {
        let not = lsp_server::Notification::new(N::METHOD.to_owned(), params);
        self.send(not.into());
    }
    fn send(&self, message : Message) {
        self.connection.sender.send(message).expect("Failed to send a message");
    }

    pub fn join(&mut self) -> std::io::Result<()> {
        match self.io_threads.take() {
            Some(h) => h.join(),
            None => Ok(()),
        }
    }
}


#[allow(non_snake_case)]
#[derive(Debug, Serialize, Deserialize)]
struct ValidateFileResponse {
    IsOk: bool,
    Messages: Vec<ValidateFileError>,
}

#[allow(non_snake_case)]
#[derive(Debug, Serialize, Deserialize)]
struct Quit {}

impl ValidateFileResponse {
    fn ok() -> Self {
        Self {
            IsOk: true,
            Messages: Vec::new(),
        }
    }
    fn error(error_list: &ShaderErrorList) -> Self {
        use crate::shader_error::ShaderError;
        let mut errors = Vec::new();
        for error in &error_list.errors {
            errors.push(match error {
                ShaderError::ParserErr {
                    filename,
                    severity,
                    error,
                    line,
                    pos,
                } => ValidateFileError::ParserErr {
                    filename: filename.clone(),
                    severity: severity.to_string(),
                    error: error.clone(),
                    line: *line as u32,
                    pos: *pos,
                },
                ShaderError::ValidationErr { message } => ValidateFileError::ValidationErr {
                    message: message.clone(),
                },
                ShaderError::InternalErr(error) => ValidateFileError::UnknownError(error.clone()),
                ShaderError::IoErr(error) => ValidateFileError::UnknownError(error.to_string()),
            });
        }
        Self {
            IsOk: false,
            Messages: errors,
        }
    }
}

pub fn get_validator(shading_language: ShadingLanguage) -> Box<dyn Validator> {
    // TODO: cache validator to avoid recreating them
    match shading_language {
        ShadingLanguage::Wgsl => Box::new(Naga::new()),
        ShadingLanguage::Hlsl => {
            #[cfg(target_os = "wasi")]
            {
                Box::new(Glslang::hlsl())
            }
            #[cfg(not(target_os = "wasi"))]
            {
                Box::new(Dxc::new().expect("Failed to create DXC"))
            }
        }
        ShadingLanguage::Glsl => Box::new(Glslang::glsl()),
    }
}

pub fn run() {    
    let mut server = ServerLanguage::new();

    match server.initialize() {
        Ok(()) => {},
        Err(value) => { error!("{:#?}", value); }
    }

    match server.run() {
        Ok(()) => {},
        Err(value) => { error!("{:#?}", value); }
    }

    match server.join() {
        Ok(()) => {},
        Err(value) => { error!("{:#?}", value); }
    }
}