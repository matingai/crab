use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct NetworkPolicyConfig {
    #[serde(
        default,
        alias = "allowPrivateNetwork",
        alias = "allow_private_addresses",
        alias = "allowPrivateAddresses"
    )]
    pub allow_private_network: bool,
    #[serde(default, alias = "allowedHosts", alias = "allow_hosts")]
    pub allowed_hosts: Vec<String>,
    #[serde(default, alias = "blockedHosts", alias = "block_hosts")]
    pub blocked_hosts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkPolicyPreflight {
    Allow,
    Deny(String),
}

#[derive(Debug, Deserialize, Default)]
struct RootConfig {
    #[serde(default, alias = "networkPolicy")]
    network_policy: NetworkPolicyConfig,
}

impl Default for NetworkPolicyConfig {
    fn default() -> Self {
        Self {
            allow_private_network: false,
            allowed_hosts: Vec::new(),
            blocked_hosts: Vec::new(),
        }
    }
}

impl NetworkPolicyConfig {
    fn normalized(self) -> Self {
        Self {
            allow_private_network: self.allow_private_network,
            allowed_hosts: normalize_host_patterns(self.allowed_hosts),
            blocked_hosts: normalize_host_patterns(self.blocked_hosts),
        }
    }

    pub fn is_default(&self) -> bool {
        !self.allow_private_network
            && self.allowed_hosts.is_empty()
            && self.blocked_hosts.is_empty()
    }
}

pub fn load_network_policy_config(data_dir: &Path) -> Result<NetworkPolicyConfig> {
    let Some(config_path) = existing_config_path(data_dir) else {
        return Ok(NetworkPolicyConfig::default());
    };
    let raw = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let root = serde_yaml::from_str::<RootConfig>(&raw)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;
    Ok(root.network_policy.normalized())
}

pub fn evaluate_network_policy(
    data_dir: &Path,
    tool_name: &str,
    url: &str,
) -> Result<NetworkPolicyPreflight> {
    let config = load_network_policy_config(data_dir)?;
    Ok(evaluate_network_policy_with_config(&config, tool_name, url))
}

pub fn evaluate_network_policy_with_config(
    config: &NetworkPolicyConfig,
    tool_name: &str,
    url: &str,
) -> NetworkPolicyPreflight {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return NetworkPolicyPreflight::Allow;
    };
    let Some(host) = parsed.host_str().map(normalize_host) else {
        return NetworkPolicyPreflight::Allow;
    };

    if host_matches_any(&config.blocked_hosts, &host) {
        return NetworkPolicyPreflight::Deny(format!(
            "tool `{tool_name}` is blocked by network_policy for host `{host}`"
        ));
    }

    if host_matches_any(&config.allowed_hosts, &host) {
        return NetworkPolicyPreflight::Allow;
    }

    if !config.allow_private_network && is_private_network_host(&host) {
        return NetworkPolicyPreflight::Deny(format!(
            "tool `{tool_name}` is blocked by network_policy from accessing private/local host `{host}`"
        ));
    }

    NetworkPolicyPreflight::Allow
}

pub fn is_private_network_host(host: &str) -> bool {
    let host = normalize_host(host);
    if matches!(host.as_str(), "localhost" | "0.0.0.0") {
        return true;
    }
    if host.ends_with(".localhost") || host.ends_with(".local") {
        return true;
    }
    host.parse::<IpAddr>().is_ok_and(is_restricted_ip)
}

fn is_restricted_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_restricted_ipv4(ip),
        IpAddr::V6(ip) => is_restricted_ipv6(ip),
    }
}

fn is_restricted_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 169 && octets[1] == 254)
        || (octets[0] == 198 && (18..=19).contains(&octets[1]))
}

fn is_restricted_ipv6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    ip.is_loopback()
        || ip.is_unspecified()
        || (segments[0] & 0xfe00) == 0xfc00
        || (segments[0] & 0xffc0) == 0xfe80
        || (segments[0] & 0xff00) == 0xff00
}

fn host_matches_any(patterns: &[String], host: &str) -> bool {
    patterns
        .iter()
        .any(|pattern| host_matches_pattern(pattern, host))
}

fn host_matches_pattern(pattern: &str, host: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return host == suffix || host.ends_with(&format!(".{suffix}"));
    }
    pattern == host
}

fn normalize_host_patterns(items: Vec<String>) -> Vec<String> {
    items
        .into_iter()
        .map(|item| normalize_host(&item))
        .filter(|item| !item.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn normalize_host(host: &str) -> String {
    host.trim()
        .trim_matches(['[', ']'])
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

fn existing_config_path(data_dir: &Path) -> Option<PathBuf> {
    ["config.yaml", "config.yml"]
        .iter()
        .map(|name| data_dir.join(name))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::{
        NetworkPolicyConfig, NetworkPolicyPreflight, evaluate_network_policy,
        evaluate_network_policy_with_config, is_private_network_host, load_network_policy_config,
    };
    use std::fs;

    #[test]
    fn load_missing_network_policy_uses_safe_default() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = load_network_policy_config(tmp.path()).expect("policy");

        assert!(config.is_default());
        assert!(!config.allow_private_network);
    }

    #[test]
    fn detects_private_and_local_hosts() {
        for host in [
            "localhost",
            "api.localhost",
            "service.local",
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.20",
            "169.254.169.254",
            "100.64.0.1",
            "::1",
            "fd00::1",
            "fe80::1",
        ] {
            assert!(is_private_network_host(host), "{host} should be private");
        }
        assert!(!is_private_network_host("example.com"));
        assert!(!is_private_network_host("8.8.8.8"));
    }

    #[test]
    fn default_policy_blocks_private_urls() {
        let config = NetworkPolicyConfig::default();
        let decision =
            evaluate_network_policy_with_config(&config, "web_extract", "http://127.0.0.1:8080/");

        assert!(
            matches!(decision, NetworkPolicyPreflight::Deny(reason) if reason.contains("private/local host"))
        );
    }

    #[test]
    fn allowed_hosts_override_private_default() {
        let config = NetworkPolicyConfig {
            allow_private_network: false,
            allowed_hosts: vec!["localhost".to_string()],
            blocked_hosts: Vec::new(),
        }
        .normalized();

        assert_eq!(
            evaluate_network_policy_with_config(&config, "web_extract", "http://localhost:1420/"),
            NetworkPolicyPreflight::Allow
        );
    }

    #[test]
    fn blocked_hosts_take_precedence_over_allowed_hosts() {
        let config = NetworkPolicyConfig {
            allow_private_network: true,
            allowed_hosts: vec!["*".to_string()],
            blocked_hosts: vec!["*.example.com".to_string()],
        }
        .normalized();

        let decision = evaluate_network_policy_with_config(
            &config,
            "web_extract",
            "https://docs.example.com/",
        );
        assert!(
            matches!(decision, NetworkPolicyPreflight::Deny(reason) if reason.contains("docs.example.com"))
        );
    }

    #[test]
    fn loads_and_applies_network_policy_config() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("config.yaml"),
            r#"
network_policy:
  allow_private_network: false
  allowed_hosts:
    - localhost
  blocked_hosts:
    - "*.internal.example"
"#,
        )
        .expect("write config");

        let local = evaluate_network_policy(tmp.path(), "web_extract", "http://localhost:1420/")
            .expect("local policy");
        assert_eq!(local, NetworkPolicyPreflight::Allow);

        let blocked =
            evaluate_network_policy(tmp.path(), "web_extract", "https://api.internal.example/")
                .expect("blocked policy");
        assert!(matches!(blocked, NetworkPolicyPreflight::Deny(_)));
    }
}
