//! Resolve the application proxy setting into an explicit child-process environment.
//!
//! ACP agents are separate CLI processes. They cannot use reqwest's in-process proxy
//! discovery, and GUI applications frequently do not inherit the shell proxy variables.
//! This module therefore makes the selected global setting authoritative for all eight
//! conventional upper/lower-case proxy variables before an agent is spawned.

use crate::config::ConfiguredAgent;
use std::collections::HashMap;

const HTTP_PROXY_KEYS: [&str; 2] = ["HTTP_PROXY", "http_proxy"];
const HTTPS_PROXY_KEYS: [&str; 2] = ["HTTPS_PROXY", "https_proxy"];
const ALL_PROXY_KEYS: [&str; 2] = ["ALL_PROXY", "all_proxy"];
const NO_PROXY_KEYS: [&str; 2] = ["NO_PROXY", "no_proxy"];

/// Global application proxy fields needed when starting an ACP process.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessProxySettings {
    pub proxy_type: Option<String>,
    pub address: Option<String>,
    pub port: Option<u16>,
}

/// Protocol-specific proxy values suitable for HTTP clients and child processes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProxyEnvironment {
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub all_proxy: Option<String>,
    pub no_proxy: Option<String>,
}

impl ProxyEnvironment {
    fn has_proxy(&self) -> bool {
        self.http_proxy.is_some() || self.https_proxy.is_some() || self.all_proxy.is_some()
    }
}

/// Resolve a global application proxy setting using the native system resolver when requested.
pub fn resolve_proxy_environment(
    settings: &ProcessProxySettings,
) -> anyhow::Result<ProxyEnvironment> {
    resolve_proxy_environment_with(settings, resolve_system_proxy)
}

/// Clone an agent launch with the global proxy setting encoded into its process environment.
///
/// The injected resolver keeps native proxy discovery deterministic in tests. It is called only
/// for `system`; explicit and direct settings never consult ambient system state.
pub fn configured_agent_with_proxy(
    mut agent: ConfiguredAgent,
    settings: &ProcessProxySettings,
    resolver: impl FnOnce() -> anyhow::Result<ProxyEnvironment>,
) -> anyhow::Result<ConfiguredAgent> {
    let proxy = resolve_proxy_environment_with(settings, resolver)?;
    apply_proxy_environment(&mut agent.env, &proxy);
    agent.validate()?;
    Ok(agent)
}

fn resolve_proxy_environment_with(
    settings: &ProcessProxySettings,
    resolver: impl FnOnce() -> anyhow::Result<ProxyEnvironment>,
) -> anyhow::Result<ProxyEnvironment> {
    let Some(proxy_type) = settings.proxy_type.as_deref() else {
        return Ok(ProxyEnvironment::default());
    };
    match proxy_type.trim().to_ascii_lowercase().as_str() {
        "none" => Ok(ProxyEnvironment::default()),
        "system" => normalize_proxy_environment(resolver()?),
        "http" => explicit_proxy_environment(settings, "http"),
        "socks5" => explicit_proxy_environment(settings, "socks5"),
        other => anyhow::bail!("unsupported ACP process proxy type `{other}`"),
    }
}

fn explicit_proxy_environment(
    settings: &ProcessProxySettings,
    scheme: &str,
) -> anyhow::Result<ProxyEnvironment> {
    let address = settings
        .address
        .as_deref()
        .map(str::trim)
        .filter(|address| !address.is_empty())
        .ok_or_else(|| anyhow::anyhow!("ACP {scheme} proxy address is required"))?;
    let port = settings
        .port
        .filter(|port| *port != 0)
        .ok_or_else(|| anyhow::anyhow!("ACP {scheme} proxy port is required"))?;
    let endpoint = explicit_proxy_url(scheme, address, port)?;
    Ok(ProxyEnvironment {
        http_proxy: Some(endpoint.clone()),
        https_proxy: Some(endpoint.clone()),
        all_proxy: Some(endpoint),
        no_proxy: Some(local_bypass_list(None)),
    })
}

fn explicit_proxy_url(scheme: &str, address: &str, port: u16) -> anyhow::Result<String> {
    if address.contains("://") {
        anyhow::bail!("ACP proxy address must not include a URL scheme");
    }
    if address
        .chars()
        .any(|ch| ch.is_whitespace() || ch.is_control() || "/@?#".contains(ch))
    {
        anyhow::bail!("ACP proxy address contains invalid URL characters");
    }
    let host = if address.starts_with('[') && address.ends_with(']') {
        address.to_string()
    } else if address.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{address}]")
    } else {
        address.to_string()
    };
    let value = format!("{scheme}://{host}:{port}");
    validate_proxy_url(&value)?;
    Ok(value)
}

fn normalize_proxy_environment(proxy: ProxyEnvironment) -> anyhow::Result<ProxyEnvironment> {
    let mut normalized = ProxyEnvironment {
        http_proxy: normalize_proxy_url(proxy.http_proxy, "http")?,
        https_proxy: normalize_proxy_url(proxy.https_proxy, "http")?,
        all_proxy: normalize_proxy_url(proxy.all_proxy, "socks5")?,
        no_proxy: proxy
            .no_proxy
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    };
    if normalized.has_proxy() {
        normalized.no_proxy = Some(local_bypass_list(normalized.no_proxy.as_deref()));
    }
    Ok(normalized)
}

fn local_bypass_list(existing: Option<&str>) -> String {
    let mut entries = Vec::new();
    for value in existing
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let candidates = if value.eq_ignore_ascii_case("<local>") {
            vec!["localhost", "127.0.0.1", "::1"]
        } else {
            vec![value]
        };
        for candidate in candidates {
            if !entries
                .iter()
                .any(|entry: &String| entry.eq_ignore_ascii_case(candidate))
            {
                entries.push(candidate.to_string());
            }
        }
    }
    for local in ["localhost", "127.0.0.1", "::1"] {
        if !entries
            .iter()
            .any(|value| value.eq_ignore_ascii_case(local))
        {
            entries.push(local.to_string());
        }
    }
    entries.join(",")
}

fn normalize_proxy_url(
    value: Option<String>,
    default_scheme: &str,
) -> anyhow::Result<Option<String>> {
    let Some(value) = value.map(|value| value.trim().to_string()) else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let value = if value.contains("://") {
        value
    } else {
        format!("{default_scheme}://{value}")
    };
    validate_proxy_url(&value)?;
    Ok(Some(value))
}

fn validate_proxy_url(value: &str) -> anyhow::Result<()> {
    let parsed = url::Url::parse(value)
        .map_err(|error| anyhow::anyhow!("invalid ACP proxy URL: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https" | "socks5" | "socks5h") {
        anyhow::bail!("unsupported ACP proxy URL scheme `{}`", parsed.scheme());
    }
    if parsed.host().is_none() {
        anyhow::bail!("ACP proxy URL must include a host");
    }
    Ok(())
}

fn apply_proxy_environment(env: &mut HashMap<String, String>, proxy: &ProxyEnvironment) {
    if !proxy.has_proxy() {
        insert_pair(env, HTTP_PROXY_KEYS, "");
        insert_pair(env, HTTPS_PROXY_KEYS, "");
        insert_pair(env, ALL_PROXY_KEYS, "");
        insert_pair(env, NO_PROXY_KEYS, "*");
        return;
    }
    insert_pair(
        env,
        HTTP_PROXY_KEYS,
        proxy.http_proxy.as_deref().unwrap_or(""),
    );
    insert_pair(
        env,
        HTTPS_PROXY_KEYS,
        proxy.https_proxy.as_deref().unwrap_or(""),
    );
    insert_pair(
        env,
        ALL_PROXY_KEYS,
        proxy.all_proxy.as_deref().unwrap_or(""),
    );
    insert_pair(env, NO_PROXY_KEYS, proxy.no_proxy.as_deref().unwrap_or(""));
}

fn insert_pair(env: &mut HashMap<String, String>, keys: [&str; 2], value: &str) {
    for key in keys {
        env.insert(key.to_string(), value.to_string());
    }
}

#[cfg(not(target_os = "macos"))]
fn inherited_proxy_environment() -> anyhow::Result<ProxyEnvironment> {
    proxy_environment_from_lookup(|key| match std::env::var_os(key) {
        Some(value) => value
            .into_string()
            .map(Some)
            .map_err(|_| anyhow::anyhow!("system proxy environment `{key}` is not valid UTF-8")),
        None => Ok(None),
    })
}

#[cfg(any(not(target_os = "macos"), test))]
fn proxy_environment_from_lookup(
    mut lookup: impl FnMut(&str) -> anyhow::Result<Option<String>>,
) -> anyhow::Result<ProxyEnvironment> {
    fn first(
        lookup: &mut impl FnMut(&str) -> anyhow::Result<Option<String>>,
        upper: &str,
        lower: &str,
    ) -> anyhow::Result<Option<String>> {
        let upper_value = lookup(upper)?;
        if upper_value
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            return Ok(upper_value);
        }
        lookup(lower)
    }

    normalize_proxy_environment(ProxyEnvironment {
        http_proxy: first(&mut lookup, "HTTP_PROXY", "http_proxy")?,
        https_proxy: first(&mut lookup, "HTTPS_PROXY", "https_proxy")?,
        all_proxy: first(&mut lookup, "ALL_PROXY", "all_proxy")?,
        no_proxy: first(&mut lookup, "NO_PROXY", "no_proxy")?,
    })
}

#[cfg(target_os = "macos")]
pub fn resolve_system_proxy() -> anyhow::Result<ProxyEnvironment> {
    macos::resolve()
}

#[cfg(target_os = "linux")]
pub fn resolve_system_proxy() -> anyhow::Result<ProxyEnvironment> {
    inherited_proxy_environment()
}

#[cfg(target_os = "windows")]
pub fn resolve_system_proxy() -> anyhow::Result<ProxyEnvironment> {
    windows::resolve()
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn resolve_system_proxy() -> anyhow::Result<ProxyEnvironment> {
    inherited_proxy_environment()
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{normalize_proxy_environment, ProxyEnvironment};
    use system_configuration::core_foundation::{
        array::CFArray,
        base::{CFType, CFTypeRef, TCFType},
        boolean::CFBoolean,
        dictionary::CFDictionary,
        number::CFNumber,
        string::CFString,
    };
    use system_configuration::dynamic_store::SCDynamicStoreBuilder;

    pub(super) fn resolve() -> anyhow::Result<ProxyEnvironment> {
        let store = SCDynamicStoreBuilder::new("AQBot ACP proxy resolver")
            .build()
            .ok_or_else(|| anyhow::anyhow!("failed to open macOS SystemConfiguration store"))?;
        let Some(proxies) = store.get_proxies() else {
            return Ok(ProxyEnvironment::default());
        };

        let http_proxy = endpoint(&proxies, "HTTP", "http")?;
        let https_proxy = endpoint(&proxies, "HTTPS", "http")?;
        let all_proxy = endpoint(&proxies, "SOCKS", "socks5")?;
        if http_proxy.is_none() && https_proxy.is_none() && all_proxy.is_none() {
            let pac_enabled = bool_value(&proxies, "ProxyAutoConfigEnable")
                || bool_value(&proxies, "ProxyAutoDiscoveryEnable");
            if pac_enabled {
                anyhow::bail!(
                    "macOS system proxy uses PAC/WPAD only; ACP child-process proxy variables require a static proxy"
                );
            }
        }

        normalize_proxy_environment(ProxyEnvironment {
            http_proxy,
            https_proxy,
            all_proxy,
            no_proxy: exceptions(&proxies)?,
        })
    }

    fn endpoint(
        proxies: &CFDictionary<CFString, CFType>,
        prefix: &str,
        scheme: &str,
    ) -> anyhow::Result<Option<String>> {
        if !bool_value(proxies, &format!("{prefix}Enable")) {
            return Ok(None);
        }
        let host_key = format!("{prefix}Proxy");
        let port_key = format!("{prefix}Port");
        let host = string_value(proxies, &host_key)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("macOS {prefix} proxy is enabled without a host"))?;
        let port = number_value(proxies, &port_key)
            .filter(|port| (1..=u16::MAX as i64).contains(port))
            .ok_or_else(|| anyhow::anyhow!("macOS {prefix} proxy has an invalid port"))?;
        super::explicit_proxy_url(scheme, host.trim(), port as u16).map(Some)
    }

    fn value(proxies: &CFDictionary<CFString, CFType>, key: &str) -> Option<CFType> {
        proxies
            .find(CFString::new(key))
            .map(|value| (*value).clone())
    }

    fn bool_value(proxies: &CFDictionary<CFString, CFType>, key: &str) -> bool {
        let Some(value) = value(proxies, key) else {
            return false;
        };
        value
            .downcast::<CFBoolean>()
            .map(bool::from)
            .or_else(|| {
                value
                    .downcast::<CFNumber>()
                    .and_then(|value| value.to_i64())
                    .map(|value| value != 0)
            })
            .unwrap_or(false)
    }

    fn string_value(proxies: &CFDictionary<CFString, CFType>, key: &str) -> Option<String> {
        value(proxies, key)
            .and_then(|value| value.downcast::<CFString>())
            .map(|value| value.to_string())
    }

    fn number_value(proxies: &CFDictionary<CFString, CFType>, key: &str) -> Option<i64> {
        value(proxies, key)
            .and_then(|value| value.downcast::<CFNumber>())
            .and_then(|value| value.to_i64())
    }

    fn exceptions(proxies: &CFDictionary<CFString, CFType>) -> anyhow::Result<Option<String>> {
        let Some(value) = value(proxies, "ExceptionsList") else {
            return Ok(None);
        };
        let array = value
            .downcast::<CFArray>()
            .ok_or_else(|| anyhow::anyhow!("macOS proxy exceptions have an invalid type"))?;
        let mut exceptions = Vec::new();
        for index in 0..array.len() {
            let raw = array
                .get(index)
                .ok_or_else(|| anyhow::anyhow!("macOS proxy exception index is missing"))?;
            let item = unsafe { CFType::wrap_under_get_rule(*raw as CFTypeRef) };
            let item = item
                .downcast::<CFString>()
                .ok_or_else(|| anyhow::anyhow!("macOS proxy exception is not a string"))?;
            let item = item.to_string();
            if !item.trim().is_empty() {
                exceptions.push(item.trim().to_string());
            }
        }
        if bool_value(proxies, "ExcludeSimpleHostnames") {
            exceptions.extend(["localhost".into(), "127.0.0.1".into(), "::1".into()]);
        }
        exceptions.sort();
        exceptions.dedup();
        Ok((!exceptions.is_empty()).then(|| exceptions.join(",")))
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::{inherited_proxy_environment, normalize_proxy_environment, ProxyEnvironment};
    use windows_sys::Win32::Foundation::{GetLastError, GlobalFree, ERROR_FILE_NOT_FOUND};
    use windows_sys::Win32::Networking::WinHttp::{
        WinHttpGetIEProxyConfigForCurrentUser, WINHTTP_CURRENT_USER_IE_PROXY_CONFIG,
    };

    pub(super) fn resolve() -> anyhow::Result<ProxyEnvironment> {
        let mut config: WINHTTP_CURRENT_USER_IE_PROXY_CONFIG = unsafe { std::mem::zeroed() };
        if unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut config) } == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_FILE_NOT_FOUND {
                return inherited_proxy_environment();
            }
            anyhow::bail!("WinHTTP system proxy discovery failed with error {error}");
        }

        let auto_config_url = take_wide(config.lpszAutoConfigUrl)?;
        let proxy = take_wide(config.lpszProxy)?;
        let bypass = take_wide(config.lpszProxyBypass)?;
        let uses_pac = config.fAutoDetect != 0 || auto_config_url.is_some();
        let Some(proxy) = proxy.filter(|proxy| !proxy.trim().is_empty()) else {
            if uses_pac {
                anyhow::bail!(
                    "Windows system proxy uses PAC/WPAD only; ACP child-process proxy variables require a static proxy"
                );
            }
            return inherited_proxy_environment();
        };
        let mut environment = super::parse_windows_proxy_string(&proxy)?;
        environment.no_proxy = bypass.and_then(normalize_bypass);
        normalize_proxy_environment(environment)
    }

    fn take_wide(value: *mut u16) -> anyhow::Result<Option<String>> {
        if value.is_null() {
            return Ok(None);
        }
        let mut length = 0;
        unsafe {
            while *value.add(length) != 0 {
                length += 1;
            }
        }
        let decoded = String::from_utf16(unsafe { std::slice::from_raw_parts(value, length) })
            .map_err(|error| anyhow::anyhow!("WinHTTP returned invalid UTF-16: {error}"));
        unsafe {
            GlobalFree(value.cast());
        }
        decoded.map(Some)
    }

    fn normalize_bypass(value: String) -> Option<String> {
        let values = value
            .split([';', ','])
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .flat_map(|value| {
                if value.eq_ignore_ascii_case("<local>") {
                    vec!["localhost", "127.0.0.1", "::1"]
                } else {
                    vec![value]
                }
            })
            .collect::<Vec<_>>();
        (!values.is_empty()).then(|| values.join(","))
    }
}

#[cfg(any(target_os = "windows", test))]
fn parse_windows_proxy_string(value: &str) -> anyhow::Result<ProxyEnvironment> {
    let entries = value
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();
    if entries.is_empty() {
        anyhow::bail!("Windows system proxy string is empty");
    }
    if entries.len() == 1 && !entries[0].contains('=') {
        let endpoint = normalize_proxy_url(Some(entries[0].to_string()), "http")?
            .ok_or_else(|| anyhow::anyhow!("Windows system proxy string is empty"))?;
        return Ok(ProxyEnvironment {
            http_proxy: Some(endpoint.clone()),
            https_proxy: Some(endpoint.clone()),
            all_proxy: Some(endpoint),
            no_proxy: None,
        });
    }

    let mut environment = ProxyEnvironment::default();
    for entry in entries {
        let (protocol, endpoint) = entry
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("invalid Windows system proxy entry `{entry}`"))?;
        let protocol = protocol.trim().to_ascii_lowercase();
        let default_scheme = if protocol == "socks" || protocol == "socks5" {
            "socks5"
        } else {
            "http"
        };
        let endpoint = normalize_proxy_url(Some(endpoint.trim().to_string()), default_scheme)?
            .ok_or_else(|| anyhow::anyhow!("Windows {protocol} proxy endpoint is empty"))?;
        match protocol.as_str() {
            "http" => environment.http_proxy = Some(endpoint),
            "https" => environment.https_proxy = Some(endpoint),
            "socks" | "socks5" => environment.all_proxy = Some(endpoint),
            other => anyhow::bail!("unsupported Windows system proxy protocol `{other}`"),
        }
    }
    Ok(environment)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent_with_poisoned_proxy_env() -> ConfiguredAgent {
        let mut env = HashMap::from([("TOKEN".into(), "preserved".into())]);
        for key in HTTP_PROXY_KEYS
            .into_iter()
            .chain(HTTPS_PROXY_KEYS)
            .chain(ALL_PROXY_KEYS)
            .chain(NO_PROXY_KEYS)
        {
            env.insert(key.into(), format!("poison-{key}"));
        }
        ConfiguredAgent {
            id: "proxy-test".into(),
            name: "Proxy test".into(),
            enabled: true,
            source: "custom".into(),
            command: "fake".into(),
            args: Vec::new(),
            env,
            icon: None,
            sort: 0,
        }
    }

    #[test]
    fn explicit_proxy_types_authoritatively_replace_all_proxy_keys() {
        for (proxy_type, expected) in [
            ("http", "http://127.0.0.1:7890"),
            ("socks5", "socks5://127.0.0.1:7890"),
        ] {
            let settings = ProcessProxySettings {
                proxy_type: Some(proxy_type.into()),
                address: Some("127.0.0.1".into()),
                port: Some(7890),
            };
            let agent =
                configured_agent_with_proxy(agent_with_poisoned_proxy_env(), &settings, || {
                    panic!("manual proxy must not call system resolver")
                })
                .expect("explicit proxy");
            for key in HTTP_PROXY_KEYS
                .into_iter()
                .chain(HTTPS_PROXY_KEYS)
                .chain(ALL_PROXY_KEYS)
            {
                assert_eq!(agent.env.get(key).map(String::as_str), Some(expected));
            }
            for key in NO_PROXY_KEYS {
                assert_eq!(
                    agent.env.get(key).map(String::as_str),
                    Some("localhost,127.0.0.1,::1")
                );
            }
            assert_eq!(
                agent.env.get("TOKEN").map(String::as_str),
                Some("preserved")
            );
        }
    }

    #[test]
    fn null_and_none_disable_inherited_and_agent_proxy_values() {
        for proxy_type in [None, Some("none".to_string())] {
            let settings = ProcessProxySettings {
                proxy_type,
                address: Some("ignored".into()),
                port: Some(7890),
            };
            let agent =
                configured_agent_with_proxy(agent_with_poisoned_proxy_env(), &settings, || {
                    panic!("direct mode must not call system resolver")
                })
                .expect("direct environment");
            for key in HTTP_PROXY_KEYS
                .into_iter()
                .chain(HTTPS_PROXY_KEYS)
                .chain(ALL_PROXY_KEYS)
            {
                assert_eq!(agent.env.get(key).map(String::as_str), Some(""));
            }
            for key in NO_PROXY_KEYS {
                assert_eq!(agent.env.get(key).map(String::as_str), Some("*"));
            }
            assert_eq!(
                agent.env.get("TOKEN").map(String::as_str),
                Some("preserved")
            );
        }
    }

    #[test]
    fn system_proxy_uses_resolver_and_normalizes_partial_values() {
        let settings = ProcessProxySettings {
            proxy_type: Some("system".into()),
            address: None,
            port: None,
        };
        let agent = configured_agent_with_proxy(agent_with_poisoned_proxy_env(), &settings, || {
            Ok(ProxyEnvironment {
                http_proxy: Some("proxy.local:8080".into()),
                https_proxy: None,
                all_proxy: Some("socks.local:1080".into()),
                no_proxy: Some(" localhost,.local ".into()),
            })
        })
        .expect("system environment");
        assert_eq!(agent.env["HTTP_PROXY"], "http://proxy.local:8080");
        assert_eq!(agent.env["http_proxy"], "http://proxy.local:8080");
        assert_eq!(agent.env["HTTPS_PROXY"], "");
        assert_eq!(agent.env["https_proxy"], "");
        assert_eq!(agent.env["ALL_PROXY"], "socks5://socks.local:1080");
        assert_eq!(agent.env["all_proxy"], "socks5://socks.local:1080");
        assert_eq!(agent.env["NO_PROXY"], "localhost,.local,127.0.0.1,::1");
        assert_eq!(agent.env["no_proxy"], "localhost,.local,127.0.0.1,::1");
    }

    #[test]
    fn invalid_proxy_settings_fail_explicitly() {
        for (settings, expected) in [
            (
                ProcessProxySettings {
                    proxy_type: Some("unknown".into()),
                    address: None,
                    port: None,
                },
                "unsupported ACP process proxy type",
            ),
            (
                ProcessProxySettings {
                    proxy_type: Some("http".into()),
                    address: None,
                    port: Some(7890),
                },
                "proxy address is required",
            ),
            (
                ProcessProxySettings {
                    proxy_type: Some("socks5".into()),
                    address: Some("127.0.0.1".into()),
                    port: None,
                },
                "proxy port is required",
            ),
        ] {
            let error =
                configured_agent_with_proxy(agent_with_poisoned_proxy_env(), &settings, || {
                    Ok(ProxyEnvironment::default())
                })
                .expect_err("invalid setting must fail");
            assert!(error.to_string().contains(expected), "{error}");
        }
    }

    #[test]
    fn inherited_proxy_lookup_has_stable_case_precedence_and_normalization() {
        let values = HashMap::from([
            ("HTTP_PROXY", "upper-http"),
            ("http_proxy", "lower-http"),
            ("https_proxy", "lower-https:8443"),
            ("ALL_PROXY", "socks5://upper-socks:1080"),
            ("no_proxy", " localhost "),
        ]);
        let environment = proxy_environment_from_lookup(|key| {
            Ok(values.get(key).map(|value| (*value).to_string()))
        })
        .expect("normalize inherited proxies");
        assert_eq!(environment.http_proxy.as_deref(), Some("http://upper-http"));
        assert_eq!(
            environment.https_proxy.as_deref(),
            Some("http://lower-https:8443")
        );
        assert_eq!(
            environment.all_proxy.as_deref(),
            Some("socks5://upper-socks:1080")
        );
        assert_eq!(
            environment.no_proxy.as_deref(),
            Some("localhost,127.0.0.1,::1")
        );
    }

    #[test]
    fn system_local_exception_is_expanded_and_deduplicated() {
        assert_eq!(
            local_bypass_list(Some("<local>,localhost,.corp")),
            "localhost,127.0.0.1,::1,.corp"
        );
    }

    #[test]
    fn windows_manual_proxy_syntax_maps_protocols_without_guessing() {
        let environment = parse_windows_proxy_string(
            "http=plain.local:8080;https=secure.local:8443;socks=socks.local:1080",
        )
        .expect("parse WinHTTP proxy string");
        assert_eq!(
            environment.http_proxy.as_deref(),
            Some("http://plain.local:8080")
        );
        assert_eq!(
            environment.https_proxy.as_deref(),
            Some("http://secure.local:8443")
        );
        assert_eq!(
            environment.all_proxy.as_deref(),
            Some("socks5://socks.local:1080")
        );
        assert!(parse_windows_proxy_string("ftp=legacy.local:21")
            .expect_err("unsupported protocol")
            .to_string()
            .contains("unsupported Windows system proxy protocol"));
    }
}
