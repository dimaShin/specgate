use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::io::Read;
use std::time::Duration;
use url::Url;

#[derive(Debug, Clone)]
pub enum AuthMode {
    None,
    Bearer(String),
    Basic { username: String, password: String },
    ApiKeyHeader { name: String, value: String },
    ApiKeyQuery { name: String, value: String },
}

pub fn fetch_url(url: &str, auth_mode: &AuthMode) -> Result<Vec<u8>, String> {
    let effective_url = apply_query_auth(url, auth_mode)?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .build();

    let mut request = agent.get(&effective_url);
    request = apply_header_auth(request, auth_mode);

    let response = request
        .call()
        .map_err(|error| format!("failed to fetch url {url}: {error}"))?;

    let status = response.status();
    if !(200..300).contains(&status) {
        return Err(format!("failed to fetch url {url}: http status {status}"));
    }

    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read response body from {url}: {error}"))?;

    Ok(bytes)
}

fn apply_query_auth(url: &str, auth_mode: &AuthMode) -> Result<String, String> {
    if let AuthMode::ApiKeyQuery { name, value } = auth_mode {
        let mut parsed = Url::parse(url).map_err(|error| format!("invalid url {url}: {error}"))?;
        parsed.query_pairs_mut().append_pair(name, value);
        return Ok(parsed.to_string());
    }

    Ok(url.to_string())
}

fn apply_header_auth(request: ureq::Request, auth_mode: &AuthMode) -> ureq::Request {
    match auth_mode {
        AuthMode::None | AuthMode::ApiKeyQuery { .. } => request,
        AuthMode::Bearer(token) => request.set("Authorization", &format!("Bearer {token}")),
        AuthMode::Basic { username, password } => {
            let credential = STANDARD.encode(format!("{username}:{password}"));
            request.set("Authorization", &format!("Basic {credential}"))
        }
        AuthMode::ApiKeyHeader { name, value } => request.set(name, value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_auth_is_appended_to_url() {
        let url = apply_query_auth(
            "https://example.com/openapi.json",
            &AuthMode::ApiKeyQuery {
                name: "token".to_string(),
                value: "abc".to_string(),
            },
        )
        .expect("expected URL auth to be appended");

        assert!(url.contains("token=abc"));
    }

    #[test]
    fn invalid_url_with_query_auth_fails() {
        let error = apply_query_auth(
            "not-a-url",
            &AuthMode::ApiKeyQuery {
                name: "token".to_string(),
                value: "abc".to_string(),
            },
        )
        .expect_err("expected invalid url to fail");

        assert!(error.contains("invalid url"));
    }
}
