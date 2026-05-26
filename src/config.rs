//! `regions.yaml` schema, loader, and `(env, region, service) → ResolvedTarget` resolution.
//!
//! All structs use `#[serde(deny_unknown_fields)]` so that an operator who pastes a
//! literal `client_secret: ...` (or `password: ...`, `token: ...`) into the file gets a
//! parse error — secrets must be sourced via env-var name references only.
//!
//! Two region row shapes are supported (mutually exclusive):
//! - Single `base_url:` (legacy / non-versioned services like designer/userview).
//! - Per-version `base_urls: { v1: ..., v2: ... }` map (versioned services like
//!   captain). The exec layer picks the right URL by resolved version.

use crate::error::{CliError, CliResult};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Top-level config (root document). Service name → ServiceConfig.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct RegionsConfig {
    pub services: BTreeMap<String, ServiceConfig>,
}

/// A single service section keyed by `(env)`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceConfig {
    /// env → EnvConfig.
    pub envs: BTreeMap<String, EnvConfig>,
}

/// A single (service, env) block — region map + auth + optional default_version.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvConfig {
    /// Region name (lowercased) → region row.
    pub regions: BTreeMap<String, RegionRow>,

    /// `auth: null` (or absent) marks the (service, env) as bearer-only.
    #[serde(default)]
    pub auth: Option<OAuth2Config>,

    /// Default version for a versioned service when the caller omits
    /// `--api-version`. Only meaningful when the service's endpoints are
    /// versioned; otherwise ignored.
    #[serde(default)]
    pub default_version: Option<String>,
}

/// A region row. Either a single `base_url` (flat) OR a `base_urls` map keyed
/// by version (e.g. `v1`, `v2`). Each newtype variant wraps a struct with
/// `deny_unknown_fields`, so a stray `token:` / `client_secret:` paste fails
/// the union match with an "unknown field" message.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RegionRow {
    Single(SingleBaseUrl),
    PerVersion(PerVersionBaseUrls),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SingleBaseUrl {
    pub base_url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerVersionBaseUrls {
    pub base_urls: BTreeMap<String, String>,
}

impl RegionRow {
    /// Convert the row into the resolver-facing shape (URLs trimmed).
    pub fn into_resolved(self) -> ResolvedUrls {
        match self {
            RegionRow::Single(SingleBaseUrl { base_url }) => {
                ResolvedUrls::Single(base_url.trim_end_matches('/').to_string())
            }
            RegionRow::PerVersion(PerVersionBaseUrls { base_urls }) => ResolvedUrls::PerVersion(
                base_urls
                    .into_iter()
                    .map(|(k, v)| (k, v.trim_end_matches('/').to_string()))
                    .collect(),
            ),
        }
    }

    /// Borrowing variant — useful inside the validate pass.
    pub fn is_per_version_empty(&self) -> bool {
        matches!(self, RegionRow::PerVersion(PerVersionBaseUrls { base_urls }) if base_urls.is_empty())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuth2Config {
    pub grant_type: GrantType,
    pub token_url: String,
    pub client_id_env: String,
    /// Name of env var holding the client secret. Required for every callable
    /// tuple regardless of grant type (all clients in this org are confidential).
    pub client_secret_env: String,
    #[serde(default)]
    pub scope: Option<String>,
    /// For `password` grant only.
    #[serde(default)]
    pub username_env: Option<String>,
    #[serde(default)]
    pub password_env: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantType {
    ClientCredentials,
    Password,
}

/// Result of resolving a `(env, region, service)` triple.
///
/// `base_url` is the resolved URL for the requested version (or the single
/// URL when the service is non-versioned). `default_version` is propagated
/// for the exec layer to consult.
#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    pub service: String,
    pub env: String,
    pub region: String,
    /// Per-version map for versioned services, or a single-entry sentinel for
    /// flat services. The exec layer picks `base_url` from this map by the
    /// resolved version (or the empty key `""` for flat services).
    pub urls: ResolvedUrls,
    /// `None` means the tuple is bearer-only — no OAuth flow defined.
    pub auth: Option<OAuth2Config>,
    /// Configured default version for the (env, service) — applies only to
    /// versioned services.
    pub default_version: Option<String>,
}

/// Either a single URL (flat service) or a per-version map (versioned).
#[derive(Debug, Clone)]
pub enum ResolvedUrls {
    Single(String),
    PerVersion(BTreeMap<String, String>),
}

impl ResolvedUrls {
    /// Pick the URL for the given (optional) version. For flat services,
    /// `version` is ignored. For versioned services, the version key must be
    /// present.
    pub fn pick(&self, version: Option<&str>) -> Option<&str> {
        match self {
            ResolvedUrls::Single(u) => Some(u.as_str()),
            ResolvedUrls::PerVersion(m) => version.and_then(|v| m.get(v).map(|s| s.as_str())),
        }
    }
}

impl RegionsConfig {
    /// Resolve `--config` / `$GOCTL_CONFIG` / XDG / `~/.config/goctl/regions.yaml` /
    /// `dirs::config_dir()` to a concrete path. Distinguishes "file does not exist"
    /// (Usage exit 2) from "file malformed" (Config exit 1).
    pub fn load(config_flag: Option<&Path>) -> CliResult<Self> {
        let (path, source) = resolve_config_path(config_flag)?;

        if !path.exists() {
            return Err(CliError::usage(format!(
                "config file not found at {} (source: {}); copy regions.example.yaml to this location",
                path.display(),
                source
            )));
        }

        let raw = std::fs::read_to_string(&path)
            .map_err(|e| CliError::config(format!("failed to read {}: {e}", path.display())))?;
        let cfg: RegionsConfig = serde_yaml::from_str(&raw)
            .map_err(|e| CliError::config(format!("parse error in {}: {e}", path.display())))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Used by tests / `--config <path>` directly.
    pub fn load_from(path: &Path) -> CliResult<Self> {
        if !path.exists() {
            return Err(CliError::usage(format!(
                "config file not found at {}; copy regions.example.yaml to this location",
                path.display()
            )));
        }
        let raw = std::fs::read_to_string(path)
            .map_err(|e| CliError::config(format!("failed to read {}: {e}", path.display())))?;
        let cfg: RegionsConfig = serde_yaml::from_str(&raw)
            .map_err(|e| CliError::config(format!("parse error in {}: {e}", path.display())))?;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> CliResult<()> {
        const ALLOWED_SERVICES: &[&str] = &["captain", "designer", "userview"];
        for (svc, sc) in &self.services {
            if !ALLOWED_SERVICES.contains(&svc.as_str()) {
                return Err(CliError::config(format!(
                    "unknown service `{svc}` in regions.yaml (allowed: {})",
                    ALLOWED_SERVICES.join(", ")
                )));
            }
            if sc.envs.is_empty() {
                return Err(CliError::config(format!(
                    "service `{svc}` has no envs configured"
                )));
            }
            for (env, ec) in &sc.envs {
                if ec.regions.is_empty() {
                    return Err(CliError::config(format!(
                        "service `{svc}` env `{env}` has empty `regions: {{}}` map"
                    )));
                }
                for (region, row) in &ec.regions {
                    if row.is_per_version_empty() {
                        return Err(CliError::config(format!(
                            "service `{svc}` env `{env}` region `{region}`: `base_urls` is empty"
                        )));
                    }
                }
                if let Some(auth) = &ec.auth {
                    auth.validate(svc, env)?;
                }
            }
        }
        Ok(())
    }

    /// Resolve a target. `region` matching is case-insensitive.
    pub fn resolve(
        &self,
        service: &str,
        env: &str,
        region: Option<&str>,
    ) -> CliResult<ResolvedTarget> {
        let sc = self.services.get(service).ok_or_else(|| {
            CliError::usage(format!(
                "service `{service}` not found in config (services: {})",
                self.services.keys().cloned().collect::<Vec<_>>().join(", ")
            ))
        })?;
        let ec = sc.envs.get(env).ok_or_else(|| {
            CliError::usage(format!(
                "service `{service}` has no `{env}` env (envs: {})",
                sc.envs.keys().cloned().collect::<Vec<_>>().join(", ")
            ))
        })?;

        let region_key = match region {
            Some(r) => {
                let needle = r.to_ascii_lowercase();
                ec.regions
                    .keys()
                    .find(|k| k.to_ascii_lowercase() == needle)
                    .cloned()
                    .ok_or_else(|| {
                        CliError::usage(format!(
                            "region `{r}` not found for `{service}` in `{env}` (regions: {})",
                            ec.regions.keys().cloned().collect::<Vec<_>>().join(", ")
                        ))
                    })?
            }
            None => {
                if ec.regions.len() == 1 {
                    ec.regions.keys().next().unwrap().clone()
                } else {
                    return Err(CliError::usage(format!(
                        "`--region` is required for `{service}` in `{env}` (regions: {})",
                        ec.regions.keys().cloned().collect::<Vec<_>>().join(", ")
                    )));
                }
            }
        };

        let row = ec.regions.get(&region_key).unwrap();
        let urls = row.clone().into_resolved();

        Ok(ResolvedTarget {
            service: service.to_string(),
            env: env.to_string(),
            region: region_key,
            urls,
            auth: ec.auth.clone(),
            default_version: ec.default_version.clone(),
        })
    }
}

impl OAuth2Config {
    fn validate(&self, svc: &str, env: &str) -> CliResult<()> {
        if self.token_url.is_empty() {
            return Err(CliError::config(format!(
                "service `{svc}` env `{env}`: auth.token_url is empty"
            )));
        }
        if self.client_id_env.is_empty() {
            return Err(CliError::config(format!(
                "service `{svc}` env `{env}`: auth.client_id_env is empty"
            )));
        }
        if matches!(self.grant_type, GrantType::Password)
            && (self.username_env.is_none() || self.password_env.is_none())
        {
            return Err(CliError::config(format!(
                "service `{svc}` env `{env}`: password grant requires username_env and password_env"
            )));
        }
        Ok(())
    }
}

fn resolve_config_path(config_flag: Option<&Path>) -> CliResult<(PathBuf, &'static str)> {
    if let Some(p) = config_flag {
        return Ok((p.to_path_buf(), "--config"));
    }
    if let Some(p) = std::env::var_os("GOCTL_CONFIG") {
        return Ok((PathBuf::from(p), "$GOCTL_CONFIG"));
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        let p = PathBuf::from(xdg).join("goctl/regions.yaml");
        if p.exists() {
            return Ok((p, "$XDG_CONFIG_HOME"));
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let p = PathBuf::from(home).join(".config/goctl/regions.yaml");
        if p.exists() {
            return Ok((p, "$HOME/.config"));
        }
    }
    if let Some(dir) = dirs::config_dir() {
        let p = dir.join("goctl/regions.yaml");
        return Ok((p, "dirs::config_dir"));
    }
    Err(CliError::usage(
        "cannot resolve config path; set $GOCTL_CONFIG or --config".to_string(),
    ))
}
