//! Error types, exit codes, and `--json-errors` serializer.

use serde::Serialize;
use std::fmt;
use std::process::ExitCode;

/// Stable CLI exit codes. Spec contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ExitKind {
    Config,
    Usage,
    Auth,
    UpstreamClient,
    UpstreamServer,
    Network,
}

impl ExitKind {
    pub fn code(self) -> u8 {
        match self {
            // Network shares the upstream-server code (transient) but has its own kind in JSON.
            ExitKind::Config => 1,
            ExitKind::Usage => 2,
            ExitKind::Auth => 3,
            ExitKind::UpstreamClient => 4,
            ExitKind::UpstreamServer => 5,
            ExitKind::Network => 5,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ExitKind::Config => "Config",
            ExitKind::Usage => "Usage",
            ExitKind::Auth => "Auth",
            ExitKind::UpstreamClient => "UpstreamClient",
            ExitKind::UpstreamServer => "UpstreamServer",
            ExitKind::Network => "Network",
        }
    }
}

/// Top-level CLI error envelope.
#[derive(Debug)]
pub struct CliError {
    pub kind: ExitKind,
    pub message: String,
    pub context: Option<String>,
}

impl CliError {
    pub fn new(kind: ExitKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            context: None,
        }
    }

    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        self.context = Some(ctx.into());
        self
    }

    pub fn config(msg: impl Into<String>) -> Self {
        Self::new(ExitKind::Config, msg)
    }
    pub fn usage(msg: impl Into<String>) -> Self {
        Self::new(ExitKind::Usage, msg)
    }
    pub fn auth(msg: impl Into<String>) -> Self {
        Self::new(ExitKind::Auth, msg)
    }
    pub fn upstream_client(msg: impl Into<String>) -> Self {
        Self::new(ExitKind::UpstreamClient, msg)
    }
    pub fn upstream_server(msg: impl Into<String>) -> Self {
        Self::new(ExitKind::UpstreamServer, msg)
    }
    pub fn network(msg: impl Into<String>) -> Self {
        Self::new(ExitKind::Network, msg)
    }

    pub fn exit_code(&self) -> ExitCode {
        ExitCode::from(self.kind.code())
    }

    /// Serialize as single-line JSON for `--json-errors`.
    pub fn to_json_line(&self) -> String {
        #[derive(Serialize)]
        struct Repr<'a> {
            schema_version: &'a str,
            exit_code: u8,
            kind: &'a str,
            message: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            context: Option<&'a str>,
        }
        let repr = Repr {
            schema_version: "1",
            exit_code: self.kind.code(),
            kind: self.kind.as_str(),
            message: &self.message,
            context: self.context.as_deref(),
        };
        serde_json::to_string(&repr).unwrap_or_else(|_| String::from("{\"schema_version\":\"1\"}"))
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.as_str(), self.message)?;
        if let Some(c) = &self.context {
            write!(f, " ({c})")?;
        }
        Ok(())
    }
}

impl std::error::Error for CliError {}

pub type CliResult<T> = Result<T, CliError>;
