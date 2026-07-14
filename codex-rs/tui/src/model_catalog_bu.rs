//! Browser Use Terminal-style multi-provider model catalog.
//!
//! This is a small, data-only module that mirrors the grouped model window
//! shown by the browser-use Terminal app. Backend routing happens through
//! LiteLLM (`wire_api = "chat"`), so selecting one of these models only needs
//! to swap the model string; the session's provider stays `litellm`.
//!
//! The `openai` group is intentionally *not* defined here — it is merged in at
//! render time from the live Codex GPT presets (`model_catalog`) so the real
//! model slugs and reasoning-effort options stay authoritative.

/// One selectable model row in the grouped picker.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BuModelEntry {
    /// Human-friendly name shown in the left column (e.g. "Claude Opus 4.8").
    pub display_name: &'static str,
    /// Model string sent to the backend (LiteLLM routes it).
    pub model_id: &'static str,
    /// Lowercase group key ("anthropic"/"google"/"openrouter"/"deepseek"/"recommended").
    pub provider_group: &'static str,
    /// Provider label (e.g. "Anthropic"/"Google"/"OpenRouter"). Shown as the
    /// second column for `recommended` rows and folded into the search value.
    pub provider_label: &'static str,
}

/// A titled group of model entries.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BuModelGroup {
    /// Lowercase section header (e.g. "recommended", "anthropic").
    pub key: &'static str,
    pub entries: &'static [BuModelEntry],
}

const RECOMMENDED: &[BuModelEntry] = &[
    BuModelEntry {
        display_name: "GPT-5.5",
        model_id: "openai/gpt-5.5",
        provider_group: "recommended",
        provider_label: "OpenRouter",
    },
    BuModelEntry {
        display_name: "Claude Opus 4.8",
        model_id: "claude-opus-4-8",
        provider_group: "recommended",
        provider_label: "Anthropic",
    },
    BuModelEntry {
        display_name: "Claude Fable 5",
        model_id: "claude-fable-5",
        provider_group: "recommended",
        provider_label: "Anthropic",
    },
    BuModelEntry {
        display_name: "Gemini 3.1 Pro",
        model_id: "gemini-3.1-pro-preview",
        provider_group: "recommended",
        provider_label: "Google",
    },
];

const ANTHROPIC: &[BuModelEntry] = &[
    BuModelEntry {
        display_name: "Claude Sonnet 4.6",
        model_id: "claude-sonnet-4-6",
        provider_group: "anthropic",
        provider_label: "Anthropic",
    },
    BuModelEntry {
        display_name: "Claude Opus 4.8",
        model_id: "claude-opus-4-8",
        provider_group: "anthropic",
        provider_label: "Anthropic",
    },
    BuModelEntry {
        display_name: "Claude Fable 5",
        model_id: "claude-fable-5",
        provider_group: "anthropic",
        provider_label: "Anthropic",
    },
    BuModelEntry {
        display_name: "Claude Haiku 4.5",
        model_id: "claude-haiku-4-5",
        provider_group: "anthropic",
        provider_label: "Anthropic",
    },
];

const GOOGLE: &[BuModelEntry] = &[
    BuModelEntry {
        display_name: "Gemini 3.1 Pro Preview",
        model_id: "gemini-3.1-pro-preview",
        provider_group: "google",
        provider_label: "Google",
    },
    BuModelEntry {
        display_name: "Gemini 3.5 Flash",
        model_id: "gemini-3.5-flash",
        provider_group: "google",
        provider_label: "Google",
    },
    BuModelEntry {
        display_name: "Gemini 3 Flash Preview",
        model_id: "gemini-3-flash-preview",
        provider_group: "google",
        provider_label: "Google",
    },
    BuModelEntry {
        display_name: "Gemini 3.1 Flash-Lite",
        model_id: "gemini-3.1-flash-lite",
        provider_group: "google",
        provider_label: "Google",
    },
];

const OPENROUTER: &[BuModelEntry] = &[
    BuModelEntry {
        display_name: "Qwen3.6 Plus",
        model_id: "qwen/qwen3.6-plus",
        provider_group: "openrouter",
        provider_label: "OpenRouter",
    },
    BuModelEntry {
        display_name: "Kimi K2.5",
        model_id: "moonshotai/kimi-k2.5",
        provider_group: "openrouter",
        provider_label: "OpenRouter",
    },
    BuModelEntry {
        display_name: "GLM-5",
        model_id: "z-ai/glm-5",
        provider_group: "openrouter",
        provider_label: "OpenRouter",
    },
    BuModelEntry {
        display_name: "GLM-4.7",
        model_id: "z-ai/glm-4.7",
        provider_group: "openrouter",
        provider_label: "OpenRouter",
    },
    BuModelEntry {
        display_name: "MiniMax M2.5",
        model_id: "minimax/minimax-m2.5",
        provider_group: "openrouter",
        provider_label: "OpenRouter",
    },
];

const DEEPSEEK: &[BuModelEntry] = &[BuModelEntry {
    display_name: "DeepSeek V4 Pro",
    model_id: "deepseek/deepseek-v4-pro",
    provider_group: "deepseek",
    provider_label: "DeepSeek",
}];

/// Grouped model entries in display order: recommended, anthropic, google,
/// openrouter, deepseek. The `openai` group is appended by the caller from the
/// live Codex GPT presets.
pub(crate) fn grouped() -> &'static [BuModelGroup] {
    const GROUPS: &[BuModelGroup] = &[
        BuModelGroup {
            key: "recommended",
            entries: RECOMMENDED,
        },
        BuModelGroup {
            key: "anthropic",
            entries: ANTHROPIC,
        },
        BuModelGroup {
            key: "google",
            entries: GOOGLE,
        },
        BuModelGroup {
            key: "openrouter",
            entries: OPENROUTER,
        },
        BuModelGroup {
            key: "deepseek",
            entries: DEEPSEEK,
        },
    ];
    GROUPS
}

impl BuModelEntry {
    /// Lowercased search value folding name, group and id so fuzzy typing hits.
    pub(crate) fn search_value(&self) -> String {
        format!(
            "{} {} {} {}",
            self.display_name.to_lowercase(),
            self.provider_group.to_lowercase(),
            self.provider_label.to_lowercase(),
            self.model_id.to_lowercase(),
        )
    }
}
