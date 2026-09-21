#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model {
    pub upstream: &'static str,
    pub label: &'static str,
    /// Desktop alias for Claude Desktop (pre-transform request routing).
    pub desktop_alias: &'static str,
    /// Friendly display label used in UI lists.
    pub desktop_label: &'static str,
    /// Native alias accepted by ChatGPT / Codex Desktop.
    pub codex_desktop_alias: &'static str,
    /// Reasoning controls exposed by ChatGPT / Codex Desktop for this alias.
    pub codex_desktop_reasoning_efforts: &'static [&'static str],
    pub score: Option<u8>,
    pub context: Option<u64>,
    pub output: Option<u64>,
    pub reasoning: bool,
    /// API-verified reasoning effort values accepted by the upstream model,
    /// in display order. Empty when the model has no reasoning effort control.
    pub reasoning_efforts: &'static [&'static str],
    pub vision: bool,
    pub audio: bool,
    pub video: bool,
    pub native_claude: bool,
    /// Effort level the proxy injects when the client cannot expose an Effort
    /// control for this route. Claude Desktop only renders Effort for routes on
    /// effort-capable Anthropic IDs (Opus 4.5/4.6/4.7/4.8, Sonnet 4.6), and
    /// there are fewer of those than catalog models. Routes parked on older IDs
    /// get their effort forced here instead.
    pub forced_effort: Option<&'static str>,
}

pub const FABLE_MODEL: &str = "claude-fable-5-1";
pub const OPUS_MODEL: &str = "claude-opus-5";
pub const SONNET_MODEL: &str = "claude-sonnet-5";
pub const HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";

pub const CODEX_DEFAULT_MODEL: &str = "gpt-6-astra";

// Stable server-routed tiers for OpenCode/Hermes, strongest to lightest.
// Gateway owns upstream backends, pools, fallback, and effort translation;
// client config never changes when backends swap. Combo IDs must be bare:
// a slash makes the gateway resolve provider credentials instead of a combo.
pub const GIGACHAD_MODEL: &str = "gigachad";
pub const CHAD_MODEL: &str = "chad";
pub const SIGMA_MODEL: &str = "sigma";
pub const ALPHA_MODEL: &str = "alpha";

// ponytail: fixed desktop slots; change IDs only when app catalogs change.
// Upstream routing and effort translation belong to agent.auranion.com.
pub const CLAUDE_DESKTOP_MODELS: &[(&str, &str)] = &[
    ("claude-fable-5-1", "Claude Fable 5.1"),
    ("claude-opus-5", "Claude Opus 5"),
    ("claude-sonnet-5", "Claude Sonnet 5"),
    ("claude-haiku-4-5-20251001", "Claude Haiku 4.5"),
];

pub const CODEX_DESKTOP_MODELS: &[&str] = &[
    "gpt-6-astra",
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
];

pub const MODELS: &[Model] = &[
    Model {
        upstream: GIGACHAD_MODEL,
        label: "Gigachad",
        desktop_alias: "auranion-gigachad",
        desktop_label: "Gigachad",
        codex_desktop_alias: "auranion-gigachad",
        codex_desktop_reasoning_efforts: &[],
        score: None,
        context: Some(256_000),
        output: Some(64_000),
        reasoning: true,
        reasoning_efforts: &[],
        vision: true,
        audio: false,
        video: false,
        native_claude: false,
        forced_effort: None,
    },
    Model {
        upstream: CHAD_MODEL,
        label: "Chad",
        desktop_alias: "auranion-chad",
        desktop_label: "Chad",
        codex_desktop_alias: "auranion-chad",
        codex_desktop_reasoning_efforts: &[],
        score: None,
        context: Some(256_000),
        output: Some(64_000),
        reasoning: true,
        reasoning_efforts: &[],
        vision: true,
        audio: false,
        video: false,
        native_claude: false,
        forced_effort: None,
    },
    Model {
        upstream: SIGMA_MODEL,
        label: "Sigma",
        desktop_alias: "auranion-sigma",
        desktop_label: "Sigma",
        codex_desktop_alias: "auranion-sigma",
        codex_desktop_reasoning_efforts: &[],
        score: None,
        context: Some(256_000),
        output: Some(64_000),
        reasoning: true,
        reasoning_efforts: &[],
        vision: true,
        audio: false,
        video: false,
        native_claude: false,
        forced_effort: None,
    },
    Model {
        upstream: ALPHA_MODEL,
        label: "Alpha",
        desktop_alias: "auranion-alpha",
        desktop_label: "Alpha",
        codex_desktop_alias: "auranion-alpha",
        codex_desktop_reasoning_efforts: &[],
        score: None,
        context: Some(256_000),
        output: Some(64_000),
        reasoning: true,
        reasoning_efforts: &[],
        vision: true,
        audio: false,
        video: false,
        native_claude: false,
        forced_effort: None,
    },
];

pub const CODEX_MODELS: &[Model] = &[
    Model {
        upstream: "gpt-6-astra",
        label: "GPT 6 Astra",
        desktop_alias: "claude-opus-4-8",
        desktop_label: "GPT 6 Astra",
        codex_desktop_alias: "gpt-6-astra",
        codex_desktop_reasoning_efforts: &["low", "medium", "high", "xhigh", "max", "ultra"],
        score: None,
        context: Some(1_000_000),
        output: Some(128_000),
        reasoning: true,
        reasoning_efforts: &["none", "minimal", "low", "medium", "high", "xhigh", "max"],
        vision: true,
        audio: false,
        video: false,
        native_claude: false,
        forced_effort: None,
    },
    Model {
        upstream: "gpt-5.6-sol",
        label: "GPT 5.6 Sol",
        desktop_alias: "claude-opus-4-8",
        desktop_label: "GPT 5.6 Sol",
        codex_desktop_alias: "gpt-5.6-sol",
        codex_desktop_reasoning_efforts: &["low", "medium", "high", "xhigh", "max", "ultra"],
        score: Some(59),
        context: Some(372_000),
        output: Some(128_000),
        reasoning: true,
        reasoning_efforts: &["none", "minimal", "low", "medium", "high", "xhigh", "max"],
        vision: true,
        audio: false,
        video: false,
        native_claude: false,
        forced_effort: None,
    },
    Model {
        upstream: "gpt-5.6-terra",
        label: "GPT 5.6 Terra",
        desktop_alias: "claude-opus-4-7",
        desktop_label: "GPT 5.6 Terra",
        codex_desktop_alias: "gpt-5.6-terra",
        codex_desktop_reasoning_efforts: &["low", "medium", "high", "xhigh", "max", "ultra"],
        score: Some(56),
        context: Some(272_000),
        output: Some(128_000),
        reasoning: true,
        reasoning_efforts: &["none", "minimal", "low", "medium", "high", "xhigh", "max"],
        vision: true,
        audio: false,
        video: false,
        native_claude: false,
        forced_effort: None,
    },
    Model {
        upstream: "gpt-5.6-luna",
        label: "GPT 5.6 Luna",
        desktop_alias: "claude-sonnet-4-6",
        desktop_label: "GPT 5.6 Luna",
        codex_desktop_alias: "gpt-5.6-luna",
        codex_desktop_reasoning_efforts: &["low", "medium", "high", "xhigh", "max"],
        score: Some(51),
        context: Some(272_000),
        output: Some(128_000),
        reasoning: true,
        reasoning_efforts: &["none", "minimal", "low", "medium", "high", "xhigh", "max"],
        vision: true,
        audio: false,
        video: false,
        native_claude: false,
        forced_effort: None,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn by_desktop_alias(alias: &str) -> Option<&'static Model> {
        MODELS.iter().find(|model| model.desktop_alias == alias)
    }

    fn by_codex_desktop_alias(alias: &str) -> Option<&'static Model> {
        CODEX_MODELS
            .iter()
            .find(|model| model.codex_desktop_alias == alias)
    }

    #[test]
    fn claude_code_roles_match_gateway_tiers() {
        assert_eq!(FABLE_MODEL, "claude-fable-5-1");
        assert_eq!(OPUS_MODEL, "claude-opus-5");
        assert_eq!(SONNET_MODEL, "claude-sonnet-5");
        assert_eq!(HAIKU_MODEL, "claude-haiku-4-5-20251001");
        assert_eq!(
            CLAUDE_DESKTOP_MODELS
                .iter()
                .map(|&(id, _)| id)
                .collect::<Vec<_>>(),
            [FABLE_MODEL, OPUS_MODEL, SONNET_MODEL, HAIKU_MODEL]
        );
        assert_eq!(
            CODEX_MODELS
                .iter()
                .map(|model| model.upstream)
                .collect::<Vec<_>>(),
            CODEX_DESKTOP_MODELS
        );
    }

    #[test]
    fn catalog_has_the_expected_models() {
        let upstream: HashSet<_> = MODELS.iter().map(|model| model.upstream).collect();
        let expected = [GIGACHAD_MODEL, CHAD_MODEL, SIGMA_MODEL, ALPHA_MODEL];

        assert_eq!(upstream.len(), expected.len());
        for model in expected {
            assert!(upstream.contains(model), "missing {model}");
        }
    }

    #[test]
    fn catalog_order_matches_user_priority() {
        let labels: Vec<_> = MODELS.iter().map(|model| model.label).collect();
        assert_eq!(labels, ["Gigachad", "Chad", "Sigma", "Alpha",]);
    }

    #[test]
    fn desktop_aliases_are_unique_and_complete() {
        let aliases: HashSet<_> = MODELS.iter().map(|model| model.desktop_alias).collect();
        assert_eq!(aliases.len(), MODELS.len());
        for model in MODELS {
            assert_eq!(by_desktop_alias(model.desktop_alias), Some(model));
            assert_eq!(model.desktop_label, model.label);
        }
    }

    #[test]
    fn tier_models_use_bare_gateway_combo_ids() {
        assert_eq!(
            MODELS
                .iter()
                .map(|model| model.upstream)
                .collect::<Vec<_>>(),
            ["gigachad", "chad", "sigma", "alpha"]
        );
        for model in MODELS {
            assert!(!model.upstream.contains('/'));
            assert!(model.reasoning_efforts.is_empty());
            assert_eq!(model.context, Some(256_000));
            assert_eq!(model.output, Some(64_000));
            assert!(model.forced_effort.is_none());
        }
    }

    #[test]
    fn codex_desktop_aliases_route_to_verified_targets() {
        let expected = [
            ("gpt-6-astra", "gpt-6-astra"),
            ("gpt-5.6-sol", "gpt-5.6-sol"),
            ("gpt-5.6-terra", "gpt-5.6-terra"),
            ("gpt-5.6-luna", "gpt-5.6-luna"),
        ];
        let aliases: HashSet<_> = CODEX_MODELS
            .iter()
            .map(|model| model.codex_desktop_alias)
            .collect();

        assert_eq!(aliases.len(), CODEX_MODELS.len());
        for (alias, upstream) in expected {
            assert_eq!(
                by_codex_desktop_alias(alias).map(|model| model.upstream),
                Some(upstream)
            );
        }
    }

    #[test]
    fn codex_desktop_efforts_match_verified_native_contracts() {
        let expected = [
            (
                "gpt-6-astra",
                &["low", "medium", "high", "xhigh", "max", "ultra"] as &[&str],
            ),
            (
                "gpt-5.6-sol",
                &["low", "medium", "high", "xhigh", "max", "ultra"],
            ),
            (
                "gpt-5.6-terra",
                &["low", "medium", "high", "xhigh", "max", "ultra"],
            ),
            ("gpt-5.6-luna", &["low", "medium", "high", "xhigh", "max"]),
        ];

        for (alias, efforts) in expected {
            assert_eq!(
                by_codex_desktop_alias(alias)
                    .expect("verified alias missing")
                    .codex_desktop_reasoning_efforts,
                efforts,
                "{alias} effort set mismatch"
            );
        }
    }
}
