//! Docker Engine API types (`GET /containers/json`, `GET /version`) and the mapping from a
//! container to the host ports it publishes.

use std::collections::HashMap;

use serde::Deserialize;

use crate::model::{ContainerInfo, ContainerPort, Protocol};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ApiPort {
    #[serde(rename = "IP")]
    pub ip: Option<String>,
    pub private_port: u16,
    pub public_port: Option<u16>,
    #[serde(rename = "Type")]
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ApiContainer {
    pub id: String,
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub ports: Vec<ApiPort>,
    #[serde(default)]
    pub labels: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ApiVersion {
    pub version: String,
    pub api_version: String,
    #[serde(default)]
    pub os: String,
}

pub fn parse_containers(json: &[u8]) -> Result<Vec<ContainerInfo>, String> {
    let list: Vec<ApiContainer> = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    Ok(list.into_iter().map(to_info).collect())
}

pub fn parse_version(json: &[u8]) -> Result<ApiVersion, String> {
    serde_json::from_slice(json).map_err(|e| e.to_string())
}

fn to_info(c: ApiContainer) -> ContainerInfo {
    let labels = c.labels.unwrap_or_default();
    let label = |k: &str| labels.get(k).filter(|v| !v.is_empty()).cloned();
    let mut ports: Vec<ContainerPort> = c
        .ports
        .iter()
        .filter_map(|p| {
            let protocol = match p.kind.as_str() {
                "tcp" => Protocol::Tcp,
                "udp" => Protocol::Udp,
                _ => return None,
            };
            Some(ContainerPort {
                protocol,
                host_ip: p.ip.clone().filter(|ip| !ip.is_empty()),
                host_port: p.public_port.filter(|p| *p != 0),
                container_port: p.private_port,
            })
        })
        .collect();
    ports.sort_by(|a, b| {
        (a.host_port, a.container_port, &a.host_ip).cmp(&(
            b.host_port,
            b.container_port,
            &b.host_ip,
        ))
    });
    ports.dedup();
    ContainerInfo {
        name: c
            .names
            .first()
            .map(|n| n.trim_start_matches('/').to_string())
            .unwrap_or_else(|| c.id.chars().take(12).collect()),
        id: c.id,
        image: c.image,
        state: c.state,
        status: c.status,
        compose_project: label("com.docker.compose.project"),
        compose_service: label("com.docker.compose.service"),
        compose_working_dir: label("com.docker.compose.project.working_dir"),
        ports,
    }
}

/// Distinct host ports a container publishes, per protocol.
pub fn published(info: &ContainerInfo) -> Vec<(Protocol, u16)> {
    let mut out: Vec<(Protocol, u16)> = info
        .ports
        .iter()
        .filter_map(|p| p.host_port.map(|hp| (p.protocol, hp)))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Container ids are hex; anything else is refused before it reaches a URL path.
pub fn is_valid_container_id(id: &str) -> bool {
    (12..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[
      {"Id":"4f2a9c1b7d3e8f6a5b4c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a","Names":["/shop-web-1"],"Image":"shop-web:dev",
       "State":"running","Status":"Up 3 minutes",
       "Ports":[{"IP":"0.0.0.0","PrivatePort":3000,"PublicPort":3000,"Type":"tcp"},
                {"IP":"::","PrivatePort":3000,"PublicPort":3000,"Type":"tcp"},
                {"PrivatePort":9229,"Type":"tcp"}],
       "Labels":{"com.docker.compose.project":"shop","com.docker.compose.service":"web",
                 "com.docker.compose.project.working_dir":"C:\\src\\shop"}},
      {"Id":"aaaaaaaaaaaaaaaaaaaaaaaa","Names":["/redis"],"Image":"redis:7","State":"running","Status":"Up 1 hour",
       "Ports":[{"IP":"127.0.0.1","PrivatePort":6379,"PublicPort":6379,"Type":"tcp"}],"Labels":null}
    ]"#;

    #[test]
    fn parses_containers_and_labels() {
        let list = parse_containers(SAMPLE.as_bytes()).unwrap();
        assert_eq!(list.len(), 2);
        let web = &list[0];
        assert_eq!(web.name, "shop-web-1");
        assert_eq!(web.compose_project.as_deref(), Some("shop"));
        assert_eq!(web.compose_service.as_deref(), Some("web"));
        assert_eq!(web.compose_working_dir.as_deref(), Some(r"C:\src\shop"));
        assert_eq!(web.ports.len(), 3);
        assert_eq!(published(web), vec![(Protocol::Tcp, 3000)]);
        assert_eq!(list[1].compose_project, None);
        assert_eq!(published(&list[1]), vec![(Protocol::Tcp, 6379)]);
    }

    #[test]
    fn version() {
        let v = parse_version(br#"{"Version":"28.4.0","ApiVersion":"1.51","Os":"linux"}"#).unwrap();
        assert_eq!(
            (v.version.as_str(), v.api_version.as_str()),
            ("28.4.0", "1.51")
        );
    }

    #[test]
    fn container_ids() {
        assert!(is_valid_container_id("4f2a9c1b7d3e"));
        assert!(!is_valid_container_id("../../version"));
        assert!(!is_valid_container_id("abc"));
    }
}
