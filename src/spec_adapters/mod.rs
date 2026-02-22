pub mod openapi;

#[derive(Debug, Clone, Copy)]
pub enum SpecFormat {
    Json,
    Yaml,
}

impl SpecFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            SpecFormat::Json => "json",
            SpecFormat::Yaml => "yaml",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SpecKind {
    OpenApi,
}

impl SpecKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SpecKind::OpenApi => "openapi",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpecIdentity {
    pub kind: SpecKind,
    pub declared_version: String,
}

#[derive(Debug, Clone)]
pub struct DetectedSpec {
    pub format: SpecFormat,
    pub identity: SpecIdentity,
}

pub fn detect_spec(bytes: &[u8]) -> Result<DetectedSpec, String> {
    match openapi::detect(bytes)? {
        openapi::OpenApiDetection::Supported {
            format,
            declared_version,
        } => Ok(DetectedSpec {
            format,
            identity: SpecIdentity {
                kind: SpecKind::OpenApi,
                declared_version,
            },
        }),
        openapi::OpenApiDetection::UnsupportedSwagger2 => Err(
            "unsupported protocol/version: OpenAPI 2.0 (Swagger). supported: OpenAPI 3.x. pending adapters: GraphQL, gRPC"
                .to_string(),
        ),
        openapi::OpenApiDetection::UnsupportedOpenApiVersion { declared_version } => Err(format!(
            "unsupported protocol/version: OpenAPI {declared_version}. supported: OpenAPI 3.x. pending adapters: GraphQL, gRPC"
        )),
        openapi::OpenApiDetection::NotOpenApi => Err(
            "unsupported protocol: unable to detect OpenAPI 3.x. pending adapters: GraphQL, gRPC"
                .to_string(),
        ),
    }
}
