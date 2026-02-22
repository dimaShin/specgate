use super::SpecFormat;
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
}
