use super::SpecFormat;
use super::{RuntimeRequest, RuntimeResponse, ValidationIssue, ValidationOutcome};
use serde_json::Value as JsonValue;

pub enum OpenApiDetection {
    Supported {
        format: SpecFormat,
        declared_version: String,
    },
    UnsupportedSwagger2,
    UnsupportedOpenApiVersion {
        declared_version: String,
    },
    NotOpenApi,
}

pub fn detect(bytes: &[u8]) -> Result<OpenApiDetection, String> {
    if let Ok(json_value) = serde_json::from_slice::<JsonValue>(bytes) {
        let detection = detect_json(&json_value)?;
        return Ok(match detection {
            OpenApiDetection::Supported {
                declared_version,
                ..
            } => OpenApiDetection::Supported {
                format: SpecFormat::Json,
                declared_version,
            },
            other => other,
        });
    }

    let yaml_value: serde_yaml::Value = serde_yaml::from_slice(bytes)
        .map_err(|error| format!("failed to parse spec as json or yaml: {error}"))?;
    let json_like = serde_json::to_value(yaml_value)
        .map_err(|error| format!("failed to normalize yaml value: {error}"))?;
    let detection = detect_json(&json_like)?;

    Ok(match detection {
        OpenApiDetection::Supported {
            declared_version,
            ..
        } => OpenApiDetection::Supported {
            format: SpecFormat::Yaml,
            declared_version,
        },
        other => other,
    })
}

pub fn validate_http_exchange(
    spec_bytes: &[u8],
    request: &RuntimeRequest,
    response: &RuntimeResponse,
) -> Result<ValidationOutcome, String> {
    let spec_value = parse_spec_value(spec_bytes)?;
    let spec_object = spec_value
        .as_object()
        .ok_or_else(|| "spec root must be an object".to_string())?;

    let mut outcome = ValidationOutcome::default();
    let paths = match spec_object.get("paths").and_then(JsonValue::as_object) {
        Some(value) => value,
        None => {
            outcome.issues.push(ValidationIssue {
                scope: "request".to_string(),
                message: "spec is missing paths object".to_string(),
            });
            return Ok(outcome);
        }
    };

    let matched_path_item = paths
        .iter()
        .find(|(path_template, _)| path_matches_template(path_template, &request.path))
        .map(|(_, path_item)| path_item);

    let Some(path_item) = matched_path_item else {
        outcome.issues.push(ValidationIssue {
            scope: "request".to_string(),
            message: format!("no matching operation path for request path '{}'", request.path),
        });
        return Ok(outcome);
    };

    let Some(path_object) = path_item.as_object() else {
        outcome.issues.push(ValidationIssue {
            scope: "request".to_string(),
            message: "matched path item is not an object".to_string(),
        });
        return Ok(outcome);
    };

    let method_key = request.method.to_ascii_lowercase();
    let operation = match path_object.get(method_key.as_str()) {
        Some(value) => value,
        None => {
            outcome.issues.push(ValidationIssue {
                scope: "request".to_string(),
                message: format!(
                    "no matching operation method '{}' for request path '{}'",
                    request.method, request.path
                ),
            });
            return Ok(outcome);
        }
    };

    let operation_object = match operation.as_object() {
        Some(value) => value,
        None => {
            outcome.issues.push(ValidationIssue {
                scope: "request".to_string(),
                message: "operation must be an object".to_string(),
            });
            return Ok(outcome);
        }
    };

    if request_body_required(operation_object) && request.body.is_empty() {
        outcome.issues.push(ValidationIssue {
            scope: "request".to_string(),
            message: "request body is required by spec but request body is empty".to_string(),
        });
    }

    if !request.body.is_empty() {
        let content_type = find_header(&request.headers, "content-type");
        if content_type.is_none() {
            outcome.issues.push(ValidationIssue {
                scope: "request".to_string(),
                message: "request body provided without content-type header".to_string(),
            });
        }
    }

    let status_key = response.status.to_string();
    let responses = match operation_object.get("responses").and_then(JsonValue::as_object) {
        Some(value) => value,
        None => {
            outcome.issues.push(ValidationIssue {
                scope: "response".to_string(),
                message: "operation is missing responses object".to_string(),
            });
            return Ok(outcome);
        }
    };

    if !responses.contains_key(status_key.as_str()) && !responses.contains_key("default") {
        outcome.issues.push(ValidationIssue {
            scope: "response".to_string(),
            message: format!("response status {} is not declared for operation", response.status),
        });
    } else if let Some(response_content) = declared_response_content_types(responses, &status_key) {
        if !response_content.is_empty() {
            let content_type = find_header(&response.headers, "content-type")
                .map(normalize_content_type)
                .unwrap_or_default();
            if !content_type.is_empty() && !response_content.iter().any(|item| item == &content_type)
            {
                outcome.issues.push(ValidationIssue {
                    scope: "response".to_string(),
                    message: format!(
                        "response content-type '{}' is not declared for operation",
                        content_type
                    ),
                });
            }
        }
    }

    if response.status == 204 && !response.body.is_empty() {
        outcome.issues.push(ValidationIssue {
            scope: "response".to_string(),
            message: "response status 204 should not include a response body".to_string(),
        });
    }

    Ok(outcome)
}

fn detect_json(value: &JsonValue) -> Result<OpenApiDetection, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "spec root must be an object".to_string())?;

    if let Some(openapi_version) = object.get("openapi").and_then(JsonValue::as_str) {
        if openapi_version.starts_with("3.") {
            return Ok(OpenApiDetection::Supported {
                format: SpecFormat::Json,
                declared_version: openapi_version.to_string(),
            });
        }

        return Ok(OpenApiDetection::UnsupportedOpenApiVersion {
            declared_version: openapi_version.to_string(),
        });
    }

    if object.get("swagger").and_then(JsonValue::as_str) == Some("2.0") {
        return Ok(OpenApiDetection::UnsupportedSwagger2);
    }

    Ok(OpenApiDetection::NotOpenApi)
}

fn parse_spec_value(spec_bytes: &[u8]) -> Result<JsonValue, String> {
    if let Ok(value) = serde_json::from_slice::<JsonValue>(spec_bytes) {
        return Ok(value);
    }

    let yaml_value: serde_yaml::Value = serde_yaml::from_slice(spec_bytes)
        .map_err(|error| format!("failed to parse spec as json or yaml: {error}"))?;
    serde_json::to_value(yaml_value)
        .map_err(|error| format!("failed to normalize yaml value: {error}"))
}

fn request_body_required(operation: &serde_json::Map<String, JsonValue>) -> bool {
    operation
        .get("requestBody")
        .and_then(JsonValue::as_object)
        .and_then(|request_body| request_body.get("required"))
        .and_then(JsonValue::as_bool)
        .unwrap_or(false)
}

fn find_header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.to_string())
}

fn declared_response_content_types(
    responses: &serde_json::Map<String, JsonValue>,
    status_key: &str,
) -> Option<Vec<String>> {
    let response_object = responses
        .get(status_key)
        .or_else(|| responses.get("default"))?
        .as_object()?;
    let content = response_object.get("content")?.as_object()?;

    Some(
        content
            .keys()
            .map(|key| key.to_ascii_lowercase())
            .collect(),
    )
}

fn normalize_content_type(value: String) -> String {
    value
        .split(';')
        .next()
        .map(|item| item.trim().to_ascii_lowercase())
        .unwrap_or_default()
}

fn path_matches_template(template: &str, path: &str) -> bool {
    let template_segments: Vec<&str> = template
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    let path_segments: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();

    if template_segments.len() != path_segments.len() {
        return false;
    }

    template_segments
        .iter()
        .zip(path_segments.iter())
        .all(|(template_segment, path_segment)| {
            (template_segment.starts_with('{') && template_segment.ends_with('}'))
                || template_segment == path_segment
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_openapi_3_json() {
        let bytes = br#"{"openapi":"3.0.0","info":{"title":"a","version":"1"},"paths":{}}"#;
        let detection = detect(bytes).expect("expected openapi 3 parse");

        match detection {
            OpenApiDetection::Supported {
                format,
                declared_version,
            } => {
                assert!(matches!(format, SpecFormat::Json));
                assert_eq!(declared_version, "3.0.0");
            }
            _ => panic!("expected supported openapi"),
        }
    }

    #[test]
    fn detects_swagger_2_json() {
        let bytes = br#"{"swagger":"2.0","info":{"title":"a","version":"1"},"paths":{}}"#;
        let detection = detect(bytes).expect("expected swagger parse");

        assert!(matches!(detection, OpenApiDetection::UnsupportedSwagger2));
    }

    #[test]
    fn validates_operation_and_response_status() {
        let spec = br#"{
            "openapi":"3.0.0",
            "info":{"title":"svc","version":"1"},
            "paths":{
                "/pets/{id}":{
                    "get":{
                        "responses":{"200":{"description":"ok"}}
                    }
                }
            }
        }"#;

        let request = RuntimeRequest {
            method: "GET".to_string(),
            path: "/pets/42".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        };

        let response = RuntimeResponse {
            status: 200,
            headers: Vec::new(),
            body: b"ok".to_vec(),
        };

        let outcome = validate_http_exchange(spec, &request, &response).expect("expected validation");
        assert!(outcome.is_valid());
    }

    #[test]
    fn reports_missing_path_and_status() {
        let spec = br#"{
            "openapi":"3.0.0",
            "info":{"title":"svc","version":"1"},
            "paths":{"/pets":{"get":{"responses":{"200":{"description":"ok"}}}}}
        }"#;

        let request = RuntimeRequest {
            method: "GET".to_string(),
            path: "/orders".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        };

        let response = RuntimeResponse {
            status: 201,
            headers: Vec::new(),
            body: b"created".to_vec(),
        };

        let outcome = validate_http_exchange(spec, &request, &response).expect("expected validation");
        assert!(!outcome.is_valid());
        assert!(outcome
            .issues
            .iter()
            .any(|issue| issue.message.contains("no matching operation path")));
    }
}
