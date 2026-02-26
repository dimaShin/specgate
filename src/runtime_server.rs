use crate::cli::{RuntimeCommand, RuntimeMode, RuntimeServeArgs, ValidationMode};
use crate::runtime_matching::{load_runtime_config, PrefixServiceMatcher};
use crate::spec_registry;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;
use tiny_http::{Header, Request, Response, Server, StatusCode};
use url::Url;

use crate::spec_adapters::{RuntimeRequest, RuntimeResponse};

pub fn run(command: RuntimeCommand) -> Result<String, String> {
    match command {
        RuntimeCommand::Serve(args) => run_serve(args),
    }
}

fn run_serve(args: RuntimeServeArgs) -> Result<String, String> {
    let config = load_runtime_config(&args.config_path)?;
    let matcher = PrefixServiceMatcher::from_config(config)?;
    let upstream = match (args.mode, args.upstream_url.as_deref()) {
        (RuntimeMode::Proxy, Some(url)) => {
            Some(Url::parse(url).map_err(|error| format!("invalid upstream url: {error}"))?)
        }
        (RuntimeMode::Proxy, None) => return Err("missing upstream url for proxy mode".to_string()),
        (RuntimeMode::MockPartial, Some(url)) => {
            Some(Url::parse(url).map_err(|error| format!("invalid upstream url: {error}"))?)
        }
        (RuntimeMode::MockPartial, None) => {
            return Err("missing upstream url for mock-partial mode".to_string())
        }
        (RuntimeMode::Mock, _) => None,
    };
    let mock_root = if matches!(args.mode, RuntimeMode::Mock | RuntimeMode::MockPartial) {
        Some(mock_root_dir()?)
    } else {
        None
    };

    let server = Server::http(&args.listen_addr)
        .map_err(|error| format!("failed to start runtime server on {}: {error}", args.listen_addr))?;

    let mut handled_requests = 0usize;
    let max_requests = args.max_requests;

    for mut request in server.incoming_requests() {
        let response = handle_request(
            &matcher,
            upstream.as_ref(),
            mock_root.as_ref(),
            args.validation_mode,
            args.mode,
            &mut request,
        );

        let response = match response {
            Ok(response) => response,
            Err(RuntimeServerError::MockNotFound(message)) => {
                let mut response = Response::from_string(message).with_status_code(StatusCode(404));
                add_response_header(&mut response, "x-specgate-mock", "miss");
                response
            }
            Err(RuntimeServerError::Other(error)) => {
                Response::from_string(error).with_status_code(StatusCode(502))
            }
        };

        request
            .respond(response)
            .map_err(|error| format!("failed to write runtime response: {error}"))?;

        handled_requests += 1;
        if let Some(max) = max_requests {
            if handled_requests >= max {
                break;
            }
        }
    }

    Ok(format!(
        "runtime stopped after handling {handled_requests} request(s)"
    ))
}

fn handle_request(
    matcher: &PrefixServiceMatcher,
    upstream: Option<&Url>,
    mock_root: Option<&PathBuf>,
    validation_mode: ValidationMode,
    mode: RuntimeMode,
    request: &mut Request,
) -> Result<Response<std::io::Cursor<Vec<u8>>>, RuntimeServerError> {
    let request_url = request.url().to_string();
    let request_path = request_url.split('?').next().unwrap_or(request_url.as_str());

    let incoming =
        extract_incoming_request(request, request_path).map_err(RuntimeServerError::Other)?;

    let mut matched_service_id: Option<String> = None;
    let mut matched_spec_context: Option<MatchedSpecContext> = None;
    if let Some(service_id) = matcher.match_path(request_path) {
        matched_service_id = Some(service_id.to_string());
        if let Some(active_spec) =
            spec_registry::resolve_active_spec(service_id).map_err(RuntimeServerError::Other)?
        {
            let spec_bytes = std::fs::read(&active_spec.spec_file)
                .map_err(|error| RuntimeServerError::Other(format!("failed to read active spec file: {error}")))?;

            matched_spec_context = Some(MatchedSpecContext {
                service_id: active_spec.service_id,
                digest: active_spec.digest,
                spec_kind: active_spec.spec_kind,
                declared_version: active_spec.declared_version,
                spec_bytes,
            });
        }
    }

    if let Some(context) = &matched_spec_context {
        let request_only_response = RuntimeResponse {
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
        };

        let request_validation = crate::spec_adapters::validate_http_exchange(
            &context.spec_kind,
            &context.spec_bytes,
            &incoming.to_runtime_request(),
            &request_only_response,
        )
        .map_err(RuntimeServerError::Other)?;

        if !request_validation.is_valid() && validation_mode == ValidationMode::Strict {
            let mut response = Response::from_string(validation_message(&request_validation.issues))
                .with_status_code(StatusCode(422));
            add_response_header(&mut response, "x-specgate-validation", "error");
            add_response_header(
                &mut response,
                "x-specgate-validation-issues",
                &request_validation.issues.len().to_string(),
            );
            attach_spec_context_headers(&mut response, context);
            return Ok(response);
        }
    }

    let upstream_response = match mode {
        RuntimeMode::Proxy => {
            let upstream = upstream
                .ok_or_else(|| RuntimeServerError::Other("proxy mode requires upstream url".to_string()))?;
            let target_url = build_target_url(upstream, &request_url).map_err(RuntimeServerError::Other)?;
            proxy_request(&target_url, &incoming).map_err(RuntimeServerError::Other)?
        }
        RuntimeMode::Mock => {
            let mock_root = mock_root
                .ok_or_else(|| RuntimeServerError::Other("mock mode requires mock fixture root".to_string()))?;
            let service_id = matched_service_id
                .as_deref()
                .ok_or_else(|| RuntimeServerError::MockNotFound(format!("no matching route for path: {request_path}")))?;
            load_mock_response(mock_root, service_id, &incoming.method, request_path)
                .map_err(|error| match error {
                    MockLoadError::NotFound(message) => RuntimeServerError::MockNotFound(message),
                    MockLoadError::Invalid(message) => RuntimeServerError::Other(message),
                })?
        }
        RuntimeMode::MockPartial => {
            let mock_root = mock_root
                .ok_or_else(|| RuntimeServerError::Other("mock-partial mode requires mock fixture root".to_string()))?;
            if let Some(service_id) = matched_service_id.as_deref() {
                match load_mock_response(mock_root, service_id, &incoming.method, request_path) {
                    Ok(mocked) => mocked,
                    Err(MockLoadError::NotFound(_)) => {
                        let upstream = upstream.ok_or_else(|| {
                            RuntimeServerError::Other(
                                "mock-partial mode requires upstream url".to_string(),
                            )
                        })?;
                        let target_url =
                            build_target_url(upstream, &request_url).map_err(RuntimeServerError::Other)?;
                        let proxied = proxy_request(&target_url, &incoming)
                            .map_err(RuntimeServerError::Other)?;
                        let mut response = build_tiny_response(proxied.clone());
                        add_response_header(&mut response, "x-specgate-mock", "fallback");
                        if let Some(context) = &matched_spec_context {
                            apply_validation_headers(
                                &mut response,
                                context,
                                &incoming,
                                &proxied,
                                validation_mode,
                            )?;
                        }
                        return Ok(response);
                    }
                    Err(MockLoadError::Invalid(message)) => {
                        return Err(RuntimeServerError::Other(message));
                    }
                }
            } else {
                let upstream = upstream.ok_or_else(|| {
                    RuntimeServerError::Other("mock-partial mode requires upstream url".to_string())
                })?;
                let target_url = build_target_url(upstream, &request_url).map_err(RuntimeServerError::Other)?;
                let proxied =
                    proxy_request(&target_url, &incoming).map_err(RuntimeServerError::Other)?;
                let mut response = build_tiny_response(proxied.clone());
                add_response_header(&mut response, "x-specgate-mock", "fallback");
                if let Some(context) = &matched_spec_context {
                    apply_validation_headers(
                        &mut response,
                        context,
                        &incoming,
                        &proxied,
                        validation_mode,
                    )?;
                }
                return Ok(response);
            }
        }
    };

    let mut response = build_tiny_response(upstream_response.clone());
    if mode == RuntimeMode::Mock {
        add_response_header(&mut response, "x-specgate-mock", "hit");
    } else if mode == RuntimeMode::MockPartial {
        add_response_header(&mut response, "x-specgate-mock", "hit");
    }

    if let Some(context) = &matched_spec_context {
        apply_validation_headers(
            &mut response,
            context,
            &incoming,
            &upstream_response,
            validation_mode,
        )?;
    }

    Ok(response)
}

fn apply_validation_headers(
    response: &mut Response<std::io::Cursor<Vec<u8>>>,
    context: &MatchedSpecContext,
    incoming: &IncomingRequest,
    upstream_response: &UpstreamResponse,
    validation_mode: ValidationMode,
) -> Result<(), RuntimeServerError> {
    let validation = crate::spec_adapters::validate_http_exchange(
        &context.spec_kind,
        &context.spec_bytes,
        &incoming.to_runtime_request(),
        &RuntimeResponse {
            status: upstream_response.status,
            headers: upstream_response.headers.clone(),
            body: upstream_response.body.clone(),
        },
    )
    .map_err(RuntimeServerError::Other)?;

    if validation.is_valid() {
        add_response_header(response, "x-specgate-validation", "ok");
    } else {
        add_response_header(response, "x-specgate-validation", "warn");
        add_response_header(
            response,
            "x-specgate-validation-issues",
            &validation.issues.len().to_string(),
        );

        if validation_mode == ValidationMode::Strict {
            let mut strict_response =
                Response::from_string(validation_message(&validation.issues))
                    .with_status_code(StatusCode(502));
            add_response_header(&mut strict_response, "x-specgate-validation", "error");
            add_response_header(
                &mut strict_response,
                "x-specgate-validation-issues",
                &validation.issues.len().to_string(),
            );
            attach_spec_context_headers(&mut strict_response, context);
            *response = strict_response;
            return Ok(());
        }
    }

    attach_spec_context_headers(response, context);
    Ok(())
}

fn build_target_url(upstream: &Url, request_url: &str) -> Result<String, String> {
    let (path, query) = match request_url.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (request_url, None),
    };

    let mut target = upstream.clone();
    target.set_path(path);
    target.set_query(query);
    Ok(target.to_string())
}

fn proxy_request(target_url: &str, request: &IncomingRequest) -> Result<UpstreamResponse, String> {
    let mut upstream_request = ureq::request(&request.method, target_url);

    for (header_name, header_value) in &request.headers {
        if header_name.eq_ignore_ascii_case("host")
            || header_name.eq_ignore_ascii_case("content-length")
        {
            continue;
        }

        upstream_request = upstream_request.set(header_name.as_str(), header_value.as_str());
    }

    let upstream_response = match upstream_request.send_bytes(&request.body) {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(error) => return Err(format!("upstream request failed: {error}")),
    };

    let status = upstream_response.status();
    let header_names = upstream_response.headers_names();
    let mut response_headers = Vec::new();
    for name in header_names {
        if let Some(value) = upstream_response.header(&name) {
            response_headers.push((name, value.to_string()));
        }
    }

    let mut response_body = Vec::new();
    upstream_response
        .into_reader()
        .read_to_end(&mut response_body)
        .map_err(|error| format!("failed to read upstream response body: {error}"))?;

    Ok(UpstreamResponse {
        status,
        headers: response_headers,
        body: response_body,
    })
}

fn load_mock_response(
    mock_root: &PathBuf,
    service_id: &str,
    method: &str,
    request_path: &str,
) -> Result<UpstreamResponse, MockLoadError> {
    let fixture_path = mock_root
        .join(service_id)
        .join(mock_fixture_file_name(method, request_path));
    let bytes = std::fs::read(&fixture_path).map_err(|_| {
        MockLoadError::NotFound(format!("mock fixture not found: {}", fixture_path.display()))
    })?;
    let fixture: MockFixture = serde_json::from_slice(&bytes)
        .map_err(|error| {
            MockLoadError::Invalid(format!(
                "failed to parse mock fixture {}: {error}",
                fixture_path.display()
            ))
        })?;

    let headers = fixture
        .headers
        .unwrap_or_default()
        .into_iter()
        .collect::<Vec<(String, String)>>();

    Ok(UpstreamResponse {
        status: fixture.status,
        headers,
        body: fixture.body.unwrap_or_default().into_bytes(),
    })
}

fn mock_fixture_file_name(method: &str, request_path: &str) -> String {
    let mut encoded = request_path
        .trim_start_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(sanitize_fixture_segment)
        .collect::<Vec<String>>();

    if encoded.is_empty() {
        encoded.push("root".to_string());
    }

    format!("{}__{}.json", method.to_ascii_uppercase(), encoded.join("__"))
}

fn sanitize_fixture_segment(segment: &str) -> String {
    let mut output = String::with_capacity(segment.len());
    for character in segment.chars() {
        if character.is_ascii_alphanumeric() || character == '-' || character == '_' || character == '.' {
            output.push(character);
        } else {
            output.push('_');
        }
    }
    if output.is_empty() {
        "segment".to_string()
    } else {
        output
    }
}

fn mock_root_dir() -> Result<PathBuf, String> {
    if let Ok(override_path) = std::env::var("SPECGATE_MOCK_DIR") {
        return Ok(PathBuf::from(override_path));
    }

    let base = if let Ok(registry_dir) = std::env::var("SPECGATE_REGISTRY_DIR") {
        PathBuf::from(registry_dir)
    } else {
        std::env::current_dir()
            .map_err(|error| format!("failed to get current directory: {error}"))?
            .join(".specgate")
    };

    Ok(base.join("mocks"))
}

fn build_tiny_response(upstream_response: UpstreamResponse) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut response =
        Response::from_data(upstream_response.body).with_status_code(StatusCode(upstream_response.status));
    for (name, value) in upstream_response.headers {
        if let Ok(header) = Header::from_bytes(name.as_bytes(), value.as_bytes()) {
            response.add_header(header);
        }
    }
    response
}

fn extract_incoming_request(
    request: &mut Request,
    request_path: &str,
) -> Result<IncomingRequest, String> {
    let method = request.method().as_str().to_string();
    let headers = request
        .headers()
        .iter()
        .map(|header| {
            (
                header.field.as_str().to_string(),
                header.value.as_str().to_string(),
            )
        })
        .collect();

    let mut body = Vec::new();
    request
        .as_reader()
        .read_to_end(&mut body)
        .map_err(|error| format!("failed to read request body: {error}"))?;

    Ok(IncomingRequest {
        method,
        path: request_path.to_string(),
        headers,
        body,
    })
}

fn attach_spec_context_headers(
    response: &mut Response<std::io::Cursor<Vec<u8>>>,
    context: &MatchedSpecContext,
) {
    add_response_header(response, "x-specgate-service", &context.service_id);
    add_response_header(response, "x-specgate-spec-digest", &context.digest);
    add_response_header(response, "x-specgate-spec-kind", &context.spec_kind);
    add_response_header(response, "x-specgate-spec-version", &context.declared_version);
}

fn validation_message(issues: &[crate::spec_adapters::ValidationIssue]) -> String {
    issues
        .iter()
        .map(|issue| format!("{}: {}", issue.scope, issue.message))
        .collect::<Vec<String>>()
        .join("\n")
}

#[derive(Debug, Clone)]
struct MatchedSpecContext {
    service_id: String,
    digest: String,
    spec_kind: String,
    declared_version: String,
    spec_bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct IncomingRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl IncomingRequest {
    fn to_runtime_request(&self) -> RuntimeRequest {
        RuntimeRequest {
            method: self.method.clone(),
            path: self.path.clone(),
            headers: self.headers.clone(),
            body: self.body.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct UpstreamResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize)]
struct MockFixture {
    status: u16,
    #[serde(default)]
    headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    body: Option<String>,
}

fn add_response_header(response: &mut Response<std::io::Cursor<Vec<u8>>>, name: &str, value: &str) {
    if let Ok(header) = Header::from_bytes(name.as_bytes(), value.as_bytes()) {
        response.add_header(header);
    }
}

enum RuntimeServerError {
    MockNotFound(String),
    Other(String),
}

enum MockLoadError {
    NotFound(String),
    Invalid(String),
}

