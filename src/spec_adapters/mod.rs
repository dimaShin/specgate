pub mod openapi;

#[derive(Debug, Clone)]
pub struct RuntimeRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct RuntimeResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub scope: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct ValidationOutcome {
    pub issues: Vec<ValidationIssue>,
}

impl ValidationOutcome {
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }
}

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

pub fn validate_http_exchange(
    spec_kind: &str,
    spec_bytes: &[u8],
    request: &RuntimeRequest,
    response: &RuntimeResponse,
) -> Result<ValidationOutcome, String> {
    match spec_kind {
        "openapi" => openapi::validate_http_exchange(spec_bytes, request, response),
        other => Err(format!("validation adapter not implemented for spec kind: {other}")),
    }
}
