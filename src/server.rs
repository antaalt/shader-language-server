use std::collections::HashMap;
use std::str::FromStr;
use std::{io, path::PathBuf};

use crate::common::ShadingLanguage;
use crate::common::{ValidationParams, Validator};
#[cfg(not(target_os = "wasi"))]
use crate::dxc::Dxc;
use crate::glslang::Glslang;
use crate::naga::Naga;
use crate::shader_error::ShaderErrorList;

use lsp_types::OneOf;
use lsp_types::{
    InitializeParams, ServerCapabilities,
};

use lsp_server::{Connection,  Message, Response, ResponseError};

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
        line: usize,
        pos: usize,
    },
    ValidationErr {
        message: String,
    },
    UnknownError(String),
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
                    line: *line,
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
    // Create the transport. Includes the stdio (stdin and stdout) versions but this could
    // also be implemented to use sockets or HTTP.
    let (connection, io_threads) = Connection::stdio();

    // Run the server and wait for the two threads to end (typically by trigger LSP Exit event).
    let server_capabilities = serde_json::to_value(&ServerCapabilities {
        definition_provider: Some(OneOf::Left(true)),
        ..Default::default()
    }).unwrap();

    let initialization_params = match connection.initialize(server_capabilities) {
        Ok(it) => it,
        Err(e) => {
            if e.channel_is_disconnected() {
                io_threads.join().expect("failed to join");
            }
            return;
        }
    };

    
    let _params: InitializeParams = serde_json::from_value(initialization_params).unwrap();
    eprintln!("starting example main loop");
    for msg in &connection.receiver {
        eprintln!("got msg: {msg:?}");
        match msg {
            Message::Request(req) => {
                connection.handle_shutdown(&req).expect("OUI");
                match req.method.as_str() {
                    "validate_file" => {
                        let params = ValidateFileParams::deserialize(req.params).expect("OUI");
                        let shading_language_parsed = ShadingLanguage::from_str(params.shadingLanguage.as_str());
                        let shading_language = match shading_language_parsed {
                            Ok(res) => res,
                            Err(_) => {
                                let resp = Response { id: req.id, result: None, error: Some(serde_json::from_str("Invalid shading language").expect("sf")) };
                                connection.sender.send(Message::Response(resp)).expect("OUI");
                                continue;
                            }
                        };

                        let mut validator = get_validator(shading_language);

                        let res = match validator.validate_shader(
                            &params.path,
                            &params.cwd,
                            ValidationParams::new(params.includes, params.defines),
                        ) {
                            Ok(_) => ValidateFileResponse::ok(),
                            Err(err) => ValidateFileResponse::error(&err),
                        };

                        let resp = Response { id: req.id, result: Some(serde_json::to_value(res).unwrap_or_default()), error: None };
                        connection.sender.send(Message::Response(resp)).expect("FDS");
                    }
                    _ => {
                        continue;
                    }
                }
                /*match cast::<GotoDefinition>(req) {
                    Ok((id, params)) => {
                        eprintln!("got gotoDefinition request #{id}: {params:?}");
                        let result = Some(GotoDefinitionResponse::Array(Vec::new()));
                        let result = serde_json::to_value(&result).unwrap();
                        let resp = Response { id, result: Some(result), error: None };
                        connection.sender.send(Message::Response(resp))?;
                        continue;
                    }
                    Err(err @ ExtractError::JsonError { .. }) => panic!("{err:?}"),
                    Err(ExtractError::MethodMismatch(req)) => req,
                };*/
                // ...
            }
            Message::Response(resp) => {
                eprintln!("got response: {resp:?}");
            }
            Message::Notification(not) => {
                eprintln!("got notification: {not:?}");
            }
        }
    }

    io_threads.join().expect("DFSFDSDF");

    /*let handler = IoHandler::new();
    handler.add_sync_method("get_file_tree", move |params: Params| {
        let params: ValidateFileParams = params.parse()?;
        

        let shading_language_parsed = ShadingLanguage::from_str(params.shadingLanguage.as_str());
        let shading_language = match shading_language_parsed {
            Ok(res) => res,
            Err(_) => {
                return Err(jsonrpc_core::Error::invalid_params(format!(
                    "Invalid shading language: {}",
                    params.shadingLanguage
                )));
            }
        };

        let mut validator = get_validator(shading_language);

        let tree = validator
            .get_shader_tree(
                &params.path,
                &params.cwd,
                ValidationParams::new(params.includes, params.defines),
            )
            .ok();

        Ok(serde_json::to_value(tree).unwrap_or_default())
    });

    handler.add_sync_method("validate_file", move |params: Params| {
        let params: ValidateFileParams = params.parse()?;

        let shading_language_parsed = ShadingLanguage::from_str(params.shadingLanguage.as_str());
        let shading_language = match shading_language_parsed {
            Ok(res) => res,
            Err(()) => {
                return Err(jsonrpc_core::Error::invalid_params(format!(
                    "Invalid shading language: {}",
                    params.shadingLanguage
                )));
            }
        };

        let mut validator = get_validator(shading_language);

        let res = match validator.validate_shader(
            &params.path,
            &params.cwd,
            ValidationParams::new(params.includes, params.defines),
        ) {
            Ok(_) => ValidateFileResponse::ok(),
            Err(err) => ValidateFileResponse::error(&err),
        };
        Ok(serde_json::to_value(res).unwrap_or_default())
    });
    handler.add_sync_method("quit", move |_params: Params| {
        // Simply exit server as requested.
        exit(0);
        #[allow(unreachable_code)]
        Ok(serde_json::from_str("{}").unwrap_or_default())
    });

    loop {
        for req in io::stdin().lock().lines() {
            match req {
                Ok(value) => {
                    if let Some(rsp) = handler.handle_request_sync(&value) {
                        // Send response to stdio
                        println!("{}", rsp);
                    }
                }
                Err(err) => {
                    println!(
                        "{}",
                        serde_json::to_value(err.to_string()).unwrap_or_default()
                    );
                }
            }
        }
    }*/
}
