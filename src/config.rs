//! `regions.yaml` schema, loader, and `(env, region, service) → ResolvedTarget` resolution.
//!
//! All structs use `#[serde(deny_unknown_fields)]` so that an operator who pastes a
//! literal `client_secret: ...` (or `password: ...`, `token: ...`) into the file gets a
//! parse error — secrets must be sourced via env-var name references only.

use crate::error::{CliError, CliResult};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Top-level config (root document). Service name → ServiceConfig.
///
/// We deserialize directly as a flat map (no wrapper), which lets us preserve
/// `deny_unknown_fields` semantics inside each ServiceConfig while letting
/// arbitrary top-level service keys exist.
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

/// A single (service, env) block — region map + auth.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvConfig {
    /// Region name (lowercased) → region row.
    pub regions: BTreeMap<String, RegionRow>,

    /// `auth: null` (or absent) marks the (service, env) as bearer-only.
    #[serde(default)]
    pub auth: Option<OAuth2Config>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionRow {
    pub base_url: String,
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
#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    pub service: String,
    pub env: String,
    pub region: String,
    pub base_url: String,
    /// `None` means the tuple is bearer-only — no OAuth flow defined.
    pub auth: Option<OAuth2Config>,
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
        const ALLOWED_SERVICES: &[&str] = &["captain-v1", "captain-v2", "designer", "userview"];
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
                if let Some(auth) = &ec.auth {
                    auth.validate(svc, env)?;
                }
            }
        }
        Ok(())
    }

    /// Resolve a target. `region` matching is case-insensitive.
    /// `--token-url` override is applied at the caller (CLI layer) before calling auth.
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

        Ok(ResolvedTarget {
            service: service.to_string(),
            env: env.to_string(),
            region: region_key,
            base_url: row.base_url.trim_end_matches('/').to_string(),
            auth: ec.auth.clone(),
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
