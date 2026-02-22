use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeConfig {
    pub service_routes: Vec<ServiceRoute>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceRoute {
    pub prefix: String,
    pub service_id: String,
}

pub fn load_runtime_config(path: &Path) -> Result<RuntimeConfig, String> {
    let raw = std::fs::read(path)
        .map_err(|error| format!("failed to read runtime config {}: {error}", path.display()))?;

    if path.extension().and_then(|value| value.to_str()) == Some("json") {
        serde_json::from_slice(&raw)
            .map_err(|error| format!("failed to parse runtime json config: {error}"))
    } else {
        serde_yaml::from_slice(&raw)
            .map_err(|error| format!("failed to parse runtime yaml config: {error}"))
    }
}

pub struct PrefixServiceMatcher {
    routes: Vec<NormalizedRoute>,
}

impl PrefixServiceMatcher {
    pub fn from_config(config: RuntimeConfig) -> Result<Self, String> {
        let mut routes = Vec::with_capacity(config.service_routes.len());
        for route in config.service_routes {
            routes.push(NormalizedRoute {
                prefix: normalize_prefix(&route.prefix)?,
                service_id: route.service_id,
            });
        }

        routes.sort_by(|left, right| {
            right
                .prefix
                .len()
                .cmp(&left.prefix.len())
                .then(left.prefix.cmp(&right.prefix))
                .then(left.service_id.cmp(&right.service_id))
        });

        Ok(Self { routes })
    }

    pub fn match_path<'a>(&'a self, request_path: &str) -> Option<&'a str> {
        self.routes
            .iter()
            .find(|route| matches_prefix(request_path, &route.prefix))
            .map(|route| route.service_id.as_str())
    }
}

#[derive(Debug, Clone)]
struct NormalizedRoute {
    prefix: String,
    service_id: String,
}

fn normalize_prefix(prefix: &str) -> Result<String, String> {
    if prefix.is_empty() {
        return Err("route prefix must not be empty".to_string());
    }

    if !prefix.starts_with('/') {
        return Err("route prefix must start with '/'".to_string());
    }

    if prefix == "/" {
        return Ok(prefix.to_string());
    }

    Ok(prefix.trim_end_matches('/').to_string())
}

fn matches_prefix(path: &str, prefix: &str) -> bool {
    if prefix == "/" {
        return path.starts_with('/');
    }

    if path == prefix {
        return true;
    }

    path.starts_with(prefix) && path[prefix.len()..].starts_with('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matcher_uses_longest_prefix_first() {
        let matcher = PrefixServiceMatcher::from_config(RuntimeConfig {
            service_routes: vec![
                ServiceRoute {
                    prefix: "/svc".to_string(),
                    service_id: "default".to_string(),
                },
                ServiceRoute {
                    prefix: "/svc/orders".to_string(),
                    service_id: "orders".to_string(),
                },
            ],
        })
        .expect("expected matcher");

        assert_eq!(matcher.match_path("/svc/orders/create"), Some("orders"));
        assert_eq!(matcher.match_path("/svc/customers"), Some("default"));
    }

    #[test]
    fn matcher_requires_boundary_after_prefix() {
        let matcher = PrefixServiceMatcher::from_config(RuntimeConfig {
            service_routes: vec![ServiceRoute {
                prefix: "/pet".to_string(),
                service_id: "pet".to_string(),
            }],
        })
        .expect("expected matcher");

        assert_eq!(matcher.match_path("/petstore"), None);
        assert_eq!(matcher.match_path("/pet/store"), Some("pet"));
    }

    #[test]
    fn root_prefix_matches_any_absolute_path() {
        let matcher = PrefixServiceMatcher::from_config(RuntimeConfig {
            service_routes: vec![ServiceRoute {
                prefix: "/".to_string(),
                service_id: "default".to_string(),
            }],
        })
        .expect("expected matcher");

        assert_eq!(matcher.match_path("/"), Some("default"));
        assert_eq!(matcher.match_path("/any/path"), Some("default"));
    }
}