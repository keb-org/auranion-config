use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(super) enum Integration {
    #[serde(rename = "Claude Desktop")]
    ClaudeDesktop,
    #[serde(rename = "Claude Code")]
    ClaudeCode,
    Codex,
    OpenCode,
}

impl Integration {
    pub(super) const ALL: [Self; 4] = [
        Self::ClaudeDesktop,
        Self::ClaudeCode,
        Self::Codex,
        Self::OpenCode,
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "Claude Desktop",
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
        }
    }
}
