// SPDX-FileCopyrightText: 2026 Miikka Koskinen
//
// SPDX-License-Identifier: MIT

use config::{Config, Environment, File};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    /// Raw HTML injected into every page (e.g. analytics script).
    /// Defaults to empty string if not set.
    ///
    /// This value must be operator-controlled and must never be derived
    /// from user input, as it is rendered unescaped into every page.
    #[serde(default)]
    pub html_snippet: String,
}

impl AppConfig {
    /// Load config from `beet-scheduler.toml` (optional) then
    /// environment variables prefixed with `BEET_` (e.g. `BEET_HTML_SNIPPET`).
    pub fn load() -> anyhow::Result<Self> {
        let cfg = Config::builder()
            .add_source(File::with_name("beet-scheduler").required(false))
            .add_source(Environment::with_prefix("BEET"))
            .build()?;
        Ok(cfg.try_deserialize()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_empty_snippet() {
        // nextest runs each test in its own process so the env is clean,
        // but remove just in case.
        std::env::remove_var("BEET_HTML_SNIPPET");
        let config = AppConfig::load().expect("load should succeed");
        assert_eq!(config.html_snippet, "");
    }

    #[test]
    fn reads_html_snippet_from_env_var() {
        std::env::set_var("BEET_HTML_SNIPPET", "<script>/* analytics */</script>");
        let config = AppConfig::load().expect("load should succeed");
        assert_eq!(config.html_snippet, "<script>/* analytics */</script>");
    }
}
