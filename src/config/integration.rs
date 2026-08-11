use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(super) enum Integration {
    #[serde(rename = "Claude Desktop")]
    ClaudeDesktop,
    #[serde(rename = "Claude Code")]
    ClaudeCode,
    #[serde(rename = "ChatGPT / Codex Desktop")]
    CodexDesktop,
    #[serde(rename = "Codex CLI", alias = "Codex")]
    CodexCli,
    OpenCode,
}

impl Integration {
    pub(super) const ALL: [Self; 5] = [
        Self::ClaudeDesktop,
        Self::ClaudeCode,
        Self::CodexDesktop,
        Self::CodexCli,
        Self::OpenCode,
    ];

    pub(super) const fn is_codex(self) -> bool {
        matches!(self, Self::CodexDesktop | Self::CodexCli)
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "Claude Desktop",
            Self::ClaudeCode => "Claude Code",
            Self::CodexDesktop => "ChatGPT / Codex Desktop",
            Self::CodexCli => "Codex CLI",
            Self::OpenCode => "OpenCode",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_codex_state_deserializes_as_cli() {
        assert_eq!(
            serde_json::from_str::<Integration>("\"Codex\"").unwrap(),
            Integration::CodexCli
        );
    }
}
