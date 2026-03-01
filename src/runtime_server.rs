use crate::cli::{
    MockPartialFallbackMode, RuntimeCommand, RuntimeInitMocksArgs, RuntimeMode, RuntimeServeArgs,
    ValidationMode,
};
use crate::runtime_matching::{load_runtime_config, PrefixServiceMatcher};
use crate::spec_registry;
use base64::Engine;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use tiny_http::{Header, Request, Response, Server, StatusCode};
use url::Url;

use crate::spec_adapters::{RuntimeRequest, RuntimeResponse};

pub fn run(command: RuntimeCommand) -> Result<String, String> {
    match command {
        RuntimeCommand::Serve(args) => run_serve(args),
        RuntimeCommand::InitMocks(args) => run_init_mocks(args),
    }
}

fn run_init_mocks(args: RuntimeInitMocksArgs) -> Result<String, String> {
    let config = load_runtime_config(&args.config_path)?;
    let matcher = PrefixServiceMatcher::from_config(config)?;
    let mock_root = mock_root_dir()?;
    let generated = ensure_scenario_manifests(&mock_root, &matcher)?;
    Ok(format!(
        "initialized {generated} service manifest(s) under {}",
        mock_root.display()
    ))
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

    if let Some(mock_root) = mock_root.as_ref() {
        let _ = ensure_scenario_manifests(mock_root, &matcher)?;
    }

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
            args.mock_partial_fallback,
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
    mock_partial_fallback: MockPartialFallbackMode,
    request: &mut Request,
) -> Result<Response<std::io::Cursor<Vec<u8>>>, RuntimeServerError> {
    let request_url = request.url().to_string();
    let request_path = request_url.split('?').next().unwrap_or(request_url.as_str());

    let incoming = extract_incoming_request(request, request_path, &request_url)
        .map_err(RuntimeServerError::Other)?;

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

    let mut mock_outcome: Option<&'static str> = None;
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
            let manifest = load_scenario_manifest_required(mock_root, service_id).map_err(|error| match error {
                MockLoadError::NotFound(message) => RuntimeServerError::MockNotFound(message),
                MockLoadError::Invalid(message) => RuntimeServerError::Other(message),
            })?;
            match resolve_scenario_response(mock_root, service_id, &incoming, &manifest).map_err(|error| {
                match error {
                    MockLoadError::NotFound(message) => RuntimeServerError::MockNotFound(message),
                    MockLoadError::Invalid(message) => RuntimeServerError::Other(message),
                }
            })? {
                ScenarioResolution::Hit(response) => {
                    mock_outcome = Some("hit");
                    response
                }
                ScenarioResolution::Fallback(response) => {
                    mock_outcome = Some("fallback");
                    response
                }
                ScenarioResolution::NoMatch { .. } => {
                    return Err(RuntimeServerError::MockNotFound(format!(
                        "no matching scenario for service '{service_id}' and path: {request_path}"
                    )));
                }
            }
        }
        RuntimeMode::MockPartial => {
            let mock_root = mock_root
                .ok_or_else(|| RuntimeServerError::Other("mock-partial mode requires mock fixture root".to_string()))?;
            if let Some(service_id) = matched_service_id.as_deref() {
                if let Some(manifest) = load_scenario_manifest(mock_root, service_id).map_err(|error| {
                    match error {
                        MockLoadError::NotFound(message) => RuntimeServerError::MockNotFound(message),
                        MockLoadError::Invalid(message) => RuntimeServerError::Other(message),
                    }
                })? {
                    match resolve_scenario_response(mock_root, service_id, &incoming, &manifest)
                        .map_err(|error| match error {
                            MockLoadError::NotFound(message) => RuntimeServerError::MockNotFound(message),
                            MockLoadError::Invalid(message) => RuntimeServerError::Other(message),
                        })?
                    {
                        ScenarioResolution::Hit(response) => {
                            mock_outcome = Some("hit");
                            response
                        }
                        ScenarioResolution::Fallback(response) => {
                            mock_outcome = Some("fallback");
                            response
                        }
                        ScenarioResolution::NoMatch { structural_match } => {
                            let should_fallback_upstream = match mock_partial_fallback {
                                MockPartialFallbackMode::Any => true,
                                MockPartialFallbackMode::Structural => !structural_match,
                            };

                            if should_fallback_upstream {
                                let upstream = upstream.ok_or_else(|| {
                                    RuntimeServerError::Other(
                                        "mock-partial mode requires upstream url".to_string(),
                                    )
                                })?;
                                let target_url = build_target_url(upstream, &request_url)
                                    .map_err(RuntimeServerError::Other)?;
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

                            return Err(RuntimeServerError::MockNotFound(format!(
                                "no matching scenario for service '{service_id}' and path: {request_path}"
                            )));
                        }
                    }
                } else {
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
    if let Some(mock_outcome) = mock_outcome {
        add_response_header(&mut response, "x-specgate-mock", mock_outcome);
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

fn load_scenario_manifest(
    mock_root: &PathBuf,
    service_id: &str,
) -> Result<Option<ScenarioManifest>, MockLoadError> {
    let service_dir = mock_root.join(service_id);
    let yaml_path = service_dir.join("scenarios.yaml");
    let yml_path = service_dir.join("scenarios.yml");
    let json_path = service_dir.join("scenarios.json");

    let (path, is_json) = if yaml_path.exists() {
        (yaml_path, false)
    } else if yml_path.exists() {
        (yml_path, false)
    } else if json_path.exists() {
        (json_path, true)
    } else {
        return Ok(None);
    };

    let bytes = std::fs::read(&path).map_err(|error| {
        MockLoadError::Invalid(format!(
            "failed to read scenario manifest {}: {error}",
            path.display()
        ))
    })?;

    let manifest = if is_json {
        serde_json::from_slice::<ScenarioManifest>(&bytes).map_err(|error| {
            MockLoadError::Invalid(format!(
                "failed to parse scenario manifest {}: {error}",
                path.display()
            ))
        })?
    } else {
        serde_yaml::from_slice::<ScenarioManifest>(&bytes).map_err(|error| {
            MockLoadError::Invalid(format!(
                "failed to parse scenario manifest {}: {error}",
                path.display()
            ))
        })?
    };

    Ok(Some(manifest))
}

fn load_scenario_manifest_required(
    mock_root: &PathBuf,
    service_id: &str,
) -> Result<ScenarioManifest, MockLoadError> {
    load_scenario_manifest(mock_root, service_id)?.ok_or_else(|| {
        let service_dir = mock_root.join(service_id);
        MockLoadError::NotFound(format!(
            "scenario manifest not found for service '{service_id}' in {} (expected scenarios.yaml|scenarios.yml|scenarios.json)",
            service_dir.display()
        ))
    })
}

fn resolve_scenario_response(
    mock_root: &PathBuf,
    service_id: &str,
    incoming: &IncomingRequest,
    manifest: &ScenarioManifest,
) -> Result<ScenarioResolution, MockLoadError> {
    let mut state_values = load_scenario_state(mock_root, service_id)?;

    let mut candidates = manifest
        .scenarios
        .iter()
        .enumerate()
        .collect::<Vec<(usize, &ScenarioRule)>>();

    candidates.sort_by(|(left_index, left), (right_index, right)| {
        right
            .priority
            .cmp(&left.priority)
            .then(left_index.cmp(right_index))
            .then(
                left
                    .id
                    .as_deref()
                    .unwrap_or("")
                    .cmp(right.id.as_deref().unwrap_or("")),
            )
    });

    let mut has_structural_match = false;
    for (_, rule) in candidates {
        let method_match = rule
            .when
            .method
            .as_ref()
            .map(|expected| expected.eq_ignore_ascii_case(&incoming.method))
            .unwrap_or(true);
        let path_match = rule
            .when
            .path
            .as_ref()
            .map(|expected| expected == &incoming.path)
            .unwrap_or(true);

        if !method_match || !path_match {
            continue;
        }

        if !match_state_requirements(&rule.state, &state_values) {
            continue;
        }

        has_structural_match = true;

        if !match_query_predicates(&rule.when.query, &incoming.query) {
            continue;
        }

        if !match_header_predicates(&rule.when.headers, &incoming.header_map) {
            continue;
        }

        if !match_header_predicates(&rule.when.cookies, &incoming.cookies) {
            continue;
        }

        if !match_auth_predicates(&rule.when.auth, &incoming.auth_context) {
            continue;
        }

        if !match_body_json_predicates(&rule.when.body_json, &incoming.body_json) {
            continue;
        }

        if !match_body_predicate(rule.when.body_contains.as_deref(), &incoming.body) {
            continue;
        }

        if !match_expression(rule.when.expr.as_deref(), incoming) {
            continue;
        }

        apply_state_transition(mock_root, service_id, &rule.state, &mut state_values)?;

        return Ok(ScenarioResolution::Hit(resolve_scenario_respond(
            mock_root,
            service_id,
            &rule.respond,
        )?));
    }

    if let Some(fallback) = &manifest.fallback {
        return Ok(ScenarioResolution::Fallback(resolve_scenario_respond(
            mock_root,
            service_id,
            fallback,
        )?));
    }

    if let Some(global_fallback) = load_global_fallback(mock_root)? {
        return Ok(ScenarioResolution::Fallback(global_fallback));
    }

    Ok(ScenarioResolution::NoMatch {
        structural_match: has_structural_match,
    })
}

fn load_global_fallback(mock_root: &PathBuf) -> Result<Option<UpstreamResponse>, MockLoadError> {
    let path = mock_root.join("fallback.json");
    if !path.exists() {
        return Ok(None);
    }

    let bytes = std::fs::read(&path).map_err(|error| {
        MockLoadError::Invalid(format!(
            "failed to read global fallback fixture {}: {error}",
            path.display()
        ))
    })?;
    let fixture: MockFixture = serde_json::from_slice(&bytes).map_err(|error| {
        MockLoadError::Invalid(format!(
            "failed to parse global fallback fixture {}: {error}",
            path.display()
        ))
    })?;

    Ok(Some(mock_fixture_to_upstream_response(fixture)))
}

fn resolve_scenario_respond(
    mock_root: &PathBuf,
    service_id: &str,
    respond: &ScenarioRespond,
) -> Result<UpstreamResponse, MockLoadError> {
    if let Some(fixture_name) = &respond.fixture {
        validate_fixture_reference(fixture_name)?;
        let fixture_path = mock_root.join(service_id).join(fixture_name);
        let bytes = std::fs::read(&fixture_path).map_err(|_| {
            MockLoadError::NotFound(format!(
                "mock fixture not found: {}",
                fixture_path.display()
            ))
        })?;
        let fixture: MockFixture = serde_json::from_slice(&bytes).map_err(|error| {
            MockLoadError::Invalid(format!(
                "failed to parse mock fixture {}: {error}",
                fixture_path.display()
            ))
        })?;
        return Ok(mock_fixture_to_upstream_response(fixture));
    }

    if let Some(status) = respond.status {
        return Ok(UpstreamResponse {
            status,
            headers: respond
                .headers
                .clone()
                .unwrap_or_default()
                .into_iter()
                .collect(),
            body: respond.body.clone().unwrap_or_default().into_bytes(),
        });
    }

    Err(MockLoadError::Invalid(
        "scenario respond must include either fixture or status".to_string(),
    ))
}

fn validate_fixture_reference(fixture_name: &str) -> Result<(), MockLoadError> {
    let path = Path::new(fixture_name);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(MockLoadError::Invalid(format!(
            "invalid fixture reference: {fixture_name}"
        )));
    }
    Ok(())
}

fn mock_fixture_to_upstream_response(fixture: MockFixture) -> UpstreamResponse {
    let headers = fixture
        .headers
        .unwrap_or_default()
        .into_iter()
        .collect::<Vec<(String, String)>>();

    UpstreamResponse {
        status: fixture.status,
        headers,
        body: fixture.body.unwrap_or_default().into_bytes(),
    }
}

fn match_query_predicates(
    predicates: &Option<BTreeMap<String, String>>,
    actual: &BTreeMap<String, String>,
) -> bool {
    predicates
        .as_ref()
        .map(|expected| {
            expected
                .iter()
                .all(|(name, value)| actual.get(name).map(|actual| actual == value).unwrap_or(false))
        })
        .unwrap_or(true)
}

fn match_header_predicates(
    predicates: &Option<BTreeMap<String, String>>,
    actual: &BTreeMap<String, String>,
) -> bool {
    predicates
        .as_ref()
        .map(|expected| {
            expected.iter().all(|(name, value)| {
                actual
                    .iter()
                    .find(|(actual_name, _)| actual_name.eq_ignore_ascii_case(name))
                    .map(|(_, actual_value)| actual_value == value)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(true)
}

fn match_body_predicate(body_contains: Option<&str>, body: &[u8]) -> bool {
    if let Some(fragment) = body_contains {
        return String::from_utf8_lossy(body).contains(fragment);
    }
    true
}

fn match_body_json_predicates(
    predicates: &Option<BTreeMap<String, String>>,
    body_json: &Option<Value>,
) -> bool {
    let Some(expected) = predicates.as_ref() else {
        return true;
    };

    let Some(body_json) = body_json.as_ref() else {
        return false;
    };

    expected.iter().all(|(path, value)| {
        get_json_path_value(body_json, path)
            .and_then(value_to_match_string)
            .map(|actual| actual == *value)
            .unwrap_or(false)
    })
}

fn get_json_path_value<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(value);
    }

    let mut current = value;
    for segment in path.split('.') {
        let object = current.as_object()?;
        current = object.get(segment)?;
    }

    Some(current)
}

fn match_expression(expr: Option<&str>, incoming: &IncomingRequest) -> bool {
    let Some(expr) = expr else {
        return true;
    };

    expr.split("&&")
        .map(str::trim)
        .filter(|clause| !clause.is_empty())
        .all(|clause| evaluate_expr_clause(clause, incoming).unwrap_or(false))
}

fn evaluate_expr_clause(clause: &str, incoming: &IncomingRequest) -> Option<bool> {
    let (left_raw, right_raw) = clause.split_once("==")?;
    let left = left_raw.trim();
    let right = parse_expr_literal(right_raw.trim())?;
    let actual = resolve_expr_operand(left, incoming)?;
    Some(actual == right)
}

fn parse_expr_literal(literal: &str) -> Option<String> {
    let trimmed = literal.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        return Some(trimmed[1..trimmed.len() - 1].to_string());
    }

    if trimmed.is_empty() {
        return None;
    }

    Some(trimmed.to_string())
}

fn resolve_expr_operand(operand: &str, incoming: &IncomingRequest) -> Option<String> {
    if operand == "method" {
        return Some(incoming.method.clone());
    }

    if operand == "path" {
        return Some(incoming.path.clone());
    }

    if let Some(name) = operand.strip_prefix("query.") {
        return incoming.query.get(name).cloned();
    }

    if let Some(name) = operand.strip_prefix("header.") {
        return incoming
            .header_map
            .iter()
            .find(|(actual_name, _)| actual_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone());
    }

    if let Some(name) = operand.strip_prefix("cookie.") {
        return incoming.cookies.get(name).cloned();
    }

    if operand == "auth.subject" {
        return incoming.auth_context.subject.clone();
    }

    if let Some(name) = operand.strip_prefix("auth.attr.") {
        return incoming.auth_context.attributes.get(name).cloned();
    }

    if let Some(name) = operand.strip_prefix("body.") {
        return incoming
            .body_json
            .as_ref()
            .and_then(|json| get_json_path_value(json, name))
            .and_then(value_to_match_string);
    }

    None
}

fn scenario_state_file_path(mock_root: &PathBuf, service_id: &str) -> PathBuf {
    mock_root.join("state").join(format!("{service_id}.json"))
}

fn load_scenario_state(
    mock_root: &PathBuf,
    service_id: &str,
) -> Result<BTreeMap<String, String>, MockLoadError> {
    let path = scenario_state_file_path(mock_root, service_id);
    if !path.exists() {
        return Ok(BTreeMap::new());
    }

    let bytes = std::fs::read(&path).map_err(|error| {
        MockLoadError::Invalid(format!(
            "failed to read scenario state file {}: {error}",
            path.display()
        ))
    })?;

    serde_json::from_slice(&bytes).map_err(|error| {
        MockLoadError::Invalid(format!(
            "failed to parse scenario state file {}: {error}",
            path.display()
        ))
    })
}

fn persist_scenario_state(
    path: &Path,
    values: &BTreeMap<String, String>,
) -> Result<(), MockLoadError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            MockLoadError::Invalid(format!(
                "failed to create scenario state directory {}: {error}",
                parent.display()
            ))
        })?;
    }

    let bytes = serde_json::to_vec_pretty(values)
        .map_err(|error| MockLoadError::Invalid(format!("failed to serialize scenario state: {error}")))?;

    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, bytes).map_err(|error| {
        MockLoadError::Invalid(format!(
            "failed to write scenario state temp file {}: {error}",
            tmp_path.display()
        ))
    })?;

    std::fs::rename(&tmp_path, path).map_err(|error| {
        MockLoadError::Invalid(format!(
            "failed to finalize scenario state file {}: {error}",
            path.display()
        ))
    })
}

fn state_key(state: &Option<ScenarioStateRule>) -> &str {
    state
        .as_ref()
        .and_then(|value| value.key.as_deref())
        .unwrap_or("default")
}

fn match_state_requirements(
    state: &Option<ScenarioStateRule>,
    values: &BTreeMap<String, String>,
) -> bool {
    let Some(rule_state) = state else {
        return true;
    };

    let Some(required) = rule_state.requires.as_ref() else {
        return true;
    };

    values
        .get(state_key(state))
        .map(|actual| actual == required)
        .unwrap_or(false)
}

fn apply_state_transition(
    mock_root: &PathBuf,
    service_id: &str,
    state: &Option<ScenarioStateRule>,
    values: &mut BTreeMap<String, String>,
) -> Result<(), MockLoadError> {
    let Some(rule_state) = state else {
        return Ok(());
    };

    let Some(next) = rule_state.set.as_ref() else {
        return Ok(());
    };

    let key = state_key(state).to_string();
    let needs_update = values.get(&key).map(|current| current != next).unwrap_or(true);
    if !needs_update {
        return Ok(());
    }

    values.insert(key, next.clone());
    let path = scenario_state_file_path(mock_root, service_id);
    persist_scenario_state(&path, values)
}

fn match_auth_predicates(auth: &Option<ScenarioAuthWhen>, context: &AuthContext) -> bool {
    let Some(expected) = auth.as_ref() else {
        return true;
    };

    if let Some(subject) = &expected.subject {
        if context.subject.as_ref() != Some(subject) {
            return false;
        }
    }

    if let Some(roles) = &expected.roles {
        if !roles.iter().all(|role| context.roles.contains(role.as_str())) {
            return false;
        }
    }

    if let Some(attributes) = &expected.attributes {
        if !attributes.iter().all(|(name, value)| {
            context
                .attributes
                .get(name)
                .map(|actual| actual == value)
                .unwrap_or(false)
        }) {
            return false;
        }
    }

    if let Some(expected_fingerprint) = &expected.token_fingerprint {
        if context.token_fingerprint.as_ref() != Some(expected_fingerprint) {
            return false;
        }
    }

    true
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

fn ensure_scenario_manifests(
    mock_root: &PathBuf,
    matcher: &PrefixServiceMatcher,
) -> Result<usize, String> {
    let mut generated = 0usize;
    for service_id in matcher.service_ids() {
        let service_dir = mock_root.join(&service_id);
        std::fs::create_dir_all(&service_dir).map_err(|error| {
            format!(
                "failed to create mock service directory {}: {error}",
                service_dir.display()
            )
        })?;

        let yaml_path = service_dir.join("scenarios.yaml");
        let yml_path = service_dir.join("scenarios.yml");
        let json_path = service_dir.join("scenarios.json");
        if yaml_path.exists() || yml_path.exists() || json_path.exists() {
            continue;
        }

        let service_json = serde_json::to_string(&service_id)
            .map_err(|error| format!("failed to encode service id in bootstrap manifest: {error}"))?;

        let template = format!(
            "# Auto-generated by specgate at runtime startup.\n# Edit this file to define mock behavior for service {service_id}.\n#\n# Minimal shape:\n# scenarios:\n#   - id: example\n#     priority: 10\n#     when:\n#       method: GET\n#       path: /health\n#     respond:\n#       status: 200\n#       body: '{{\"ok\":true}}'\n\nscenarios: []\n\n# Optional service-level fallback (uncomment to enable)\n# fallback:\n#   status: 501\n#   headers:\n#     content-type: application/json\n#   body: '{{\"error\":\"mock_not_configured\",\"service\":{service_json}}}'\n"
        );

        std::fs::write(&yaml_path, template).map_err(|error| {
            format!(
                "failed to write bootstrap scenario manifest {}: {error}",
                yaml_path.display()
            )
        })?;
        generated += 1;
    }

    Ok(generated)
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
    request_url: &str,
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
        header_map: build_header_map(request.headers()),
        query: parse_query_map(request_url),
        cookies: parse_cookie_map(request.headers()),
        auth_context: extract_auth_context(request.headers()),
        body_json: parse_json_body(&body),
        body,
    })
}

fn parse_json_body(body: &[u8]) -> Option<Value> {
    serde_json::from_slice::<Value>(body).ok()
}

fn build_header_map(headers: &[Header]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for header in headers {
        map.insert(
            header.field.as_str().to_ascii_lowercase().to_string(),
            header.value.as_str().to_string(),
        );
    }
    map
}

fn parse_query_map(request_url: &str) -> BTreeMap<String, String> {
    let mut query = BTreeMap::new();
    if let Some((_, query_part)) = request_url.split_once('?') {
        for (name, value) in url::form_urlencoded::parse(query_part.as_bytes()) {
            query.insert(name.into_owned(), value.into_owned());
        }
    }
    query
}

fn parse_cookie_map(headers: &[Header]) -> BTreeMap<String, String> {
    let mut cookies = BTreeMap::new();
    for header in headers {
        if header.field.equiv("Cookie") {
            for pair in header.value.as_str().split(';') {
                if let Some((name, value)) = pair.split_once('=') {
                    cookies.insert(name.trim().to_string(), value.trim().to_string());
                }
            }
        }
    }
    cookies
}

fn extract_auth_context(headers: &[Header]) -> AuthContext {
    let mut context = AuthContext::default();
    let bearer = headers.iter().find_map(|header| {
        if !header.field.equiv("Authorization") {
            return None;
        }

        let value = header.value.as_str();
        let (scheme, token) = value.split_once(' ')?;
        if scheme.eq_ignore_ascii_case("Bearer") {
            Some(token.trim().to_string())
        } else {
            None
        }
    });

    let Some(token) = bearer else {
        return context;
    };

    if token_fingerprint_enabled() {
        context.token_fingerprint = Some(token_fingerprint(&token));
    }

    let claims = decode_jwt_claims(&token);
    let Some(claims) = claims else {
        return context;
    };

    if let Some(subject) = claims.get("sub").and_then(Value::as_str) {
        context.subject = Some(subject.to_string());
    }

    if let Some(single_role) = claims.get("role").and_then(Value::as_str) {
        context.roles.insert(single_role.to_string());
    }

    if let Some(roles) = claims.get("roles").and_then(Value::as_array) {
        for role in roles {
            if let Some(role_name) = role.as_str() {
                context.roles.insert(role_name.to_string());
            }
        }
    }

    if let Some(object) = claims.as_object() {
        for (name, value) in object {
            if let Some(encoded) = value_to_match_string(value) {
                context.attributes.insert(name.clone(), encoded);
            }
        }
    }

    context
}

fn decode_jwt_claims(token: &str) -> Option<Value> {
    let mut segments = token.split('.');
    let _header = segments.next()?;
    let payload = segments.next()?;
    let _signature = segments.next()?;

    let payload_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .ok()?;

    serde_json::from_slice::<Value>(&payload_bytes).ok()
}

fn value_to_match_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some("null".to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::String(value) => Some(value.clone()),
        _ => None,
    }
}

fn token_fingerprint_enabled() -> bool {
    std::env::var("SPECGATE_TOKEN_FINGERPRINT")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn token_fingerprint(token: &str) -> String {
    let salt = std::env::var("SPECGATE_TOKEN_FINGERPRINT_SALT").unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(token.as_bytes());
    let digest = hasher.finalize();
    format!("sha256:{:x}", digest)
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
    header_map: BTreeMap<String, String>,
    query: BTreeMap<String, String>,
    cookies: BTreeMap<String, String>,
    auth_context: AuthContext,
    body_json: Option<Value>,
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

#[derive(Debug, Clone, Deserialize)]
struct ScenarioManifest {
    #[serde(default)]
    scenarios: Vec<ScenarioRule>,
    #[serde(default)]
    fallback: Option<ScenarioRespond>,
}

#[derive(Debug, Clone, Deserialize)]
struct ScenarioRule {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    priority: i64,
    #[serde(default)]
    when: ScenarioWhen,
    #[serde(default)]
    state: Option<ScenarioStateRule>,
    respond: ScenarioRespond,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ScenarioWhen {
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    query: Option<BTreeMap<String, String>>,
    #[serde(default)]
    headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    cookies: Option<BTreeMap<String, String>>,
    #[serde(default)]
    auth: Option<ScenarioAuthWhen>,
    #[serde(default)]
    body_json: Option<BTreeMap<String, String>>,
    #[serde(default)]
    body_contains: Option<String>,
    #[serde(default)]
    expr: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ScenarioAuthWhen {
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    roles: Option<Vec<String>>,
    #[serde(default)]
    attributes: Option<BTreeMap<String, String>>,
    #[serde(default)]
    token_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ScenarioStateRule {
    #[serde(default)]
    requires: Option<String>,
    #[serde(default)]
    set: Option<String>,
    #[serde(default)]
    key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ScenarioRespond {
    #[serde(default)]
    status: Option<u16>,
    #[serde(default)]
    headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    fixture: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct AuthContext {
    subject: Option<String>,
    roles: BTreeSet<String>,
    attributes: BTreeMap<String, String>,
    token_fingerprint: Option<String>,
}

enum ScenarioResolution {
    Hit(UpstreamResponse),
    Fallback(UpstreamResponse),
    NoMatch { structural_match: bool },
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

