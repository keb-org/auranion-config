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

pub const DEFAULT_MODEL: &str = "cx/gpt-5.6-sol";
pub const FABLE_MODEL: &str = "cx/gpt-5.6-sol";
pub const OPUS_MODEL: &str = "cx/gpt-5.6-terra";
pub const SONNET_MODEL: &str = "cx/gpt-5.6-luna";
pub const HAIKU_MODEL: &str = "ag/gemini-3.7-flash-tiered";

pub const MODELS: &[Model] = &[
    Model {
        upstream: "cx/gpt-5.6-sol",
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
        upstream: "cx/gpt-5.6-terra",
        label: "GPT 5.6 Terra",
        desktop_alias: "claude-sonnet-4-5-20250920",
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
        forced_effort: Some("max"),
    },
    Model {
        upstream: "cx/gpt-5.6-luna",
        label: "GPT 5.6 Luna",
        desktop_alias: "claude-sonnet-4-5",
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
        forced_effort: Some("max"),
    },
    Model {
        upstream: "gcli/grok-4.6",
        label: "Grok 4.6",
        desktop_alias: "claude-opus-4-7",
        desktop_label: "Grok 4.6",
        codex_desktop_alias: "gpt-5.3",
        codex_desktop_reasoning_efforts: &["low", "medium", "high", "xhigh", "ultra"],
        score: None,
        context: Some(500_000),
        output: Some(128_000),
        reasoning: true,
        reasoning_efforts: &["low", "medium", "high", "xhigh"],
        vision: true,
        audio: false,
        video: false,
        native_claude: false,
        forced_effort: None,
    },
    Model {
        upstream: "cmc/meta/muse-spark-1.2-contributor",
        label: "Muse Spark 1.2",
        desktop_alias: "claude-opus-4-6",
        desktop_label: "Muse Spark 1.2",
        codex_desktop_alias: "gpt-5.3-mini",
        codex_desktop_reasoning_efforts: &["minimal", "low", "medium", "high", "xhigh", "ultra"],
        score: None,
        context: Some(1_048_576),
        output: Some(128_000),
        reasoning: true,
        reasoning_efforts: &["minimal", "low", "medium", "high", "xhigh"],
        vision: true,
        audio: true,
        video: true,
        native_claude: false,
        forced_effort: None,
    },
    Model {
        upstream: "cmc/z-ai/glm-5.3-flash",
        label: "GLM 5.3 Flash",
        desktop_alias: "claude-opus-4-5-20251101",
        desktop_label: "GLM 5.3 Flash",
        codex_desktop_alias: "gpt-5.3-turbo",
        codex_desktop_reasoning_efforts: &[],
        score: None,
        context: Some(1_048_576),
        output: Some(131_072),
        reasoning: true,
        reasoning_efforts: &[],
        vision: true,
        audio: false,
        video: true,
        native_claude: false,
        forced_effort: None,
    },
    Model {
        upstream: "cmc/deepseek/deepseek-v4-flash",
        label: "DeepSeek V4 Flash",
        desktop_alias: "claude-haiku-4-5-20251001",
        desktop_label: "DeepSeek V4 Flash",
        codex_desktop_alias: "gpt-5.4-mini",
        codex_desktop_reasoning_efforts: &["low", "high", "max"],
        score: Some(50),
        context: Some(1_000_000),
        output: Some(384_000),
        reasoning: true,
        reasoning_efforts: &["low", "high", "max"],
        vision: false,
        audio: false,
        video: false,
        native_claude: false,
        forced_effort: Some("max"),
    },
    Model {
        upstream: "ag/gemini-3.7-flash-tiered",
        label: "Gemini 3.7 Flash",
        desktop_alias: "claude-haiku-4-6",
        desktop_label: "Gemini 3.7 Flash",
        codex_desktop_alias: "gpt-5.4",
        codex_desktop_reasoning_efforts: &["low", "medium", "high"],
        score: Some(50),
        context: Some(1_048_576),
        output: Some(65_536),
        reasoning: true,
        reasoning_efforts: &["low", "medium", "high"],
        vision: true,
        audio: true,
        video: true,
        native_claude: false,
        forced_effort: Some("high"),
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
        MODELS
            .iter()
            .find(|model| model.codex_desktop_alias == alias)
    }

    #[test]
    fn claude_code_roles_match_gateway_tiers() {
        assert_eq!(DEFAULT_MODEL, "cx/gpt-5.6-sol");
        assert_eq!(FABLE_MODEL, "cx/gpt-5.6-sol");
        assert_eq!(OPUS_MODEL, "cx/gpt-5.6-terra");
        assert_eq!(SONNET_MODEL, "cx/gpt-5.6-luna");
        assert_eq!(HAIKU_MODEL, "ag/gemini-3.7-flash-tiered");
    }

    #[test]
    fn catalog_has_the_expected_models() {
        let upstream: HashSet<_> = MODELS.iter().map(|model| model.upstream).collect();
        let expected = [
            "ag/gemini-3.7-flash-tiered",
            "cx/gpt-5.6-sol",
            "cx/gpt-5.6-terra",
            "cx/gpt-5.6-luna",
            "cmc/deepseek/deepseek-v4-flash",
            "cmc/meta/muse-spark-1.2-contributor",
            "gcli/grok-4.6",
            "cmc/z-ai/glm-5.3-flash",
        ];

        assert_eq!(upstream.len(), expected.len());
        for model in expected {
            assert!(upstream.contains(model), "missing {model}");
        }
    }

    #[test]
    fn catalog_order_matches_user_priority() {
        let labels: Vec<_> = MODELS.iter().map(|model| model.label).collect();
        assert_eq!(
            labels,
            [
                "GPT 5.6 Sol",
                "GPT 5.6 Terra",
                "GPT 5.6 Luna",
                "Grok 4.6",
                "Muse Spark 1.2",
                "GLM 5.3 Flash",
                "DeepSeek V4 Flash",
                "Gemini 3.7 Flash",
            ]
        );
    }

    #[test]
    fn desktop_aliases_are_unique_and_complete() {
        let aliases: HashSet<_> = MODELS.iter().map(|model| model.desktop_alias).collect();
        assert_eq!(aliases.len(), MODELS.len());
        for model in MODELS {
            assert!(model.desktop_alias.starts_with("claude-"));
            assert!(!model.desktop_alias.contains("claude-5"));
            assert_eq!(by_desktop_alias(model.desktop_alias), Some(model));
            assert_eq!(model.desktop_label, model.label);
        }
    }

    #[test]
    fn desktop_routes_preserve_non_anthropic_upstreams() {
        let gemini = by_desktop_alias("claude-haiku-4-6").unwrap();
        assert_eq!(gemini.upstream, "ag/gemini-3.7-flash-tiered");

        let sol = by_desktop_alias("claude-opus-4-8").unwrap();
        assert_eq!(sol.upstream, "cx/gpt-5.6-sol");

        let deepseek = by_desktop_alias("claude-haiku-4-5-20251001").unwrap();
        assert_eq!(deepseek.upstream, "cmc/deepseek/deepseek-v4-flash");

        let muse = by_desktop_alias("claude-opus-4-6").unwrap();
        assert_eq!(muse.upstream, "cmc/meta/muse-spark-1.2-contributor");

        let grok = by_desktop_alias("claude-opus-4-7").unwrap();
        assert_eq!(grok.upstream, "gcli/grok-4.6");
    }

    /// Claude Desktop derives the Effort control from the route model ID.
    /// Per platform.claude.com/docs/en/build-with-claude/effort, effort is
    /// supported on Opus 4.5/4.6/4.7/4.8 and Sonnet 4.6 (pre-Claude-5 set).
    /// Routes for reasoning models must use one of these or Effort disappears.
    #[test]
    fn reasoning_routes_use_effort_capable_ids() {
        const EFFORT_CAPABLE: [&str; 5] = [
            "claude-opus-4-5-20251101",
            "claude-opus-4-6",
            "claude-opus-4-7",
            "claude-opus-4-8",
            "claude-sonnet-4-6",
        ];

        for upstream in [
            "cx/gpt-5.6-sol",
            "gcli/grok-4.6",
            "cmc/meta/muse-spark-1.2-contributor",
            "cmc/z-ai/glm-5.3-flash",
        ] {
            let model = MODELS
                .iter()
                .find(|model| model.upstream == upstream)
                .unwrap_or_else(|| panic!("missing {upstream}"));
            assert!(
                EFFORT_CAPABLE.contains(&model.desktop_alias),
                "{} routes to {} which has no effort support",
                model.label,
                model.desktop_alias
            );
        }
    }

    #[test]
    fn claude_desktop_picker_routes_keep_verified_effort_mapping() {
        let expected = [
            ("cx/gpt-5.6-sol", "claude-opus-4-8"),
            ("cx/gpt-5.6-terra", "claude-sonnet-4-5-20250920"),
            ("cx/gpt-5.6-luna", "claude-sonnet-4-5"),
            ("ag/gemini-3.7-flash-tiered", "claude-haiku-4-6"),
            (
                "cmc/deepseek/deepseek-v4-flash",
                "claude-haiku-4-5-20251001",
            ),
            ("cmc/meta/muse-spark-1.2-contributor", "claude-opus-4-6"),
            ("gcli/grok-4.6", "claude-opus-4-7"),
            ("cmc/z-ai/glm-5.3-flash", "claude-opus-4-5-20251101"),
        ];

        for (upstream, desktop_alias) in expected {
            let model = MODELS
                .iter()
                .find(|model| model.upstream == upstream)
                .unwrap_or_else(|| panic!("missing {upstream}"));
            assert_eq!(model.desktop_alias, desktop_alias);
        }
    }

    /// Routes parked on non-effort-capable IDs cannot show an Effort control,
    /// so the proxy forces their effort instead. Every such route must declare
    /// one, and effort-capable routes must not (the user picks in the UI).
    #[test]
    fn non_effort_routes_declare_forced_effort() {
        const EFFORT_CAPABLE: [&str; 5] = [
            "claude-opus-4-8",
            "claude-opus-4-7",
            "claude-opus-4-6",
            "claude-opus-4-5-20251101",
            "claude-sonnet-4-6",
        ];

        for model in MODELS {
            let ui_effort = EFFORT_CAPABLE.contains(&model.desktop_alias);
            assert_eq!(
                model.forced_effort.is_none(),
                ui_effort,
                "{} routes to {} (ui_effort={}) but forced_effort={:?}",
                model.label,
                model.desktop_alias,
                ui_effort,
                model.forced_effort
            );
        }

        let model = MODELS
            .iter()
            .find(|model| model.upstream == "cmc/deepseek/deepseek-v4-flash")
            .expect("missing cmc/deepseek/deepseek-v4-flash");
        assert_eq!(model.forced_effort, Some("max"), "{}", model.label);
    }

    #[test]
    fn reasoning_efforts_match_api_verified_contracts() {
        let expected: Vec<(&str, &[&str])> = vec![
            (
                "cx/gpt-5.6-sol",
                &["none", "minimal", "low", "medium", "high", "xhigh", "max"],
            ),
            (
                "cx/gpt-5.6-terra",
                &["none", "minimal", "low", "medium", "high", "xhigh", "max"],
            ),
            (
                "cx/gpt-5.6-luna",
                &["none", "minimal", "low", "medium", "high", "xhigh", "max"],
            ),
            ("ag/gemini-3.7-flash-tiered", &["low", "medium", "high"]),
            ("cmc/deepseek/deepseek-v4-flash", &["low", "high", "max"]),
            (
                "cmc/meta/muse-spark-1.2-contributor",
                &["minimal", "low", "medium", "high", "xhigh"],
            ),
            ("gcli/grok-4.6", &["low", "medium", "high", "xhigh"]),
            ("cmc/z-ai/glm-5.3-flash", &[]),
        ];

        for (upstream, efforts) in expected {
            let model = MODELS
                .iter()
                .find(|model| model.upstream == upstream)
                .unwrap_or_else(|| panic!("missing {upstream}"));
            assert_eq!(
                model.reasoning_efforts, efforts,
                "{} effort set mismatch",
                model.label
            );
        }
    }

    #[test]
    fn codex_desktop_aliases_route_to_verified_targets() {
        let expected = [
            ("gpt-5.6-sol", "cx/gpt-5.6-sol"),
            ("gpt-5.6-terra", "cx/gpt-5.6-terra"),
            ("gpt-5.6-luna", "cx/gpt-5.6-luna"),
            ("gpt-5.4", "ag/gemini-3.7-flash-tiered"),
            ("gpt-5.4-mini", "cmc/deepseek/deepseek-v4-flash"),
            ("gpt-5.3-mini", "cmc/meta/muse-spark-1.2-contributor"),
            ("gpt-5.3", "gcli/grok-4.6"),
            ("gpt-5.3-turbo", "cmc/z-ai/glm-5.3-flash"),
        ];
        let aliases: HashSet<_> = MODELS
            .iter()
            .map(|model| model.codex_desktop_alias)
            .collect();

        assert_eq!(aliases.len(), MODELS.len());
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
                "gpt-5.6-sol",
                &["low", "medium", "high", "xhigh", "max", "ultra"] as &[&str],
            ),
            (
                "gpt-5.6-terra",
                &["low", "medium", "high", "xhigh", "max", "ultra"],
            ),
            ("gpt-5.6-luna", &["low", "medium", "high", "xhigh", "max"]),
            ("gpt-5.4", &["low", "medium", "high"]),
            ("gpt-5.4-mini", &["low", "high", "max"]),
            (
                "gpt-5.3-mini",
                &["minimal", "low", "medium", "high", "xhigh", "ultra"],
            ),
            ("gpt-5.3", &["low", "medium", "high", "xhigh", "ultra"]),
            ("gpt-5.3-turbo", &[]),
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
