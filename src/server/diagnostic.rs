use std::collections::HashMap;

use log::{error, info};
use lsp_types::{Diagnostic, PublishDiagnosticsParams, Url};

use crate::{
    server::ServerLanguage,
    shaders::{
        shader::ShadingLanguage,
        shader_error::{ShaderErrorSeverity, ValidatorError},
        validator::validator::ValidationParams,
    },
};

impl ServerLanguage {
    pub fn recolt_diagnostic(
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
                    let uri = match diagnostic.file_path {
                        Some(file_path) => {
                            Url::from_file_path(&file_path).expect(
                                format!(
                                    "Failed to convert path {} to uri",
                                    file_path.display()
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
    pub fn publish_diagnostic(
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

    pub fn clear_diagnostic(&self, uri: &Url) {
        let publish_diagnostics_params = PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics: Vec::new(),
            version: None,
        };
        self.send_notification::<lsp_types::notification::PublishDiagnostics>(
            publish_diagnostics_params,
        );
    }
}
