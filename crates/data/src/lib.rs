//! Data loading and lookup for explainshell WASM.
//!
//! The manpage corpus is exported from explainshell.db to MessagePack at
//! build time (see scripts/export_wasm_data.py) and deserialized here at
//! runtime. Lookups mirror Store.find_man_page semantics.

use std::collections::HashMap;

use explainshell_core::ParsedManpage;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors from data loading and lookup.
#[derive(Debug, Error)]
pub enum DataError {
    /// MessagePack deserialization failed
    #[error("failed to decode manpage data: {0}")]
    Decode(#[from] rmp_serde::decode::Error),

    /// Requested program has no manpage mapping
    #[error("no manpage found for program: {0}")]
    ProgramNotFound(String),
}

/// Versioned container for the exported corpus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataBundle {
    /// Schema version of the bundle format
    pub version: u32,
    /// source path -> parsed manpage
    pub manpages: HashMap<String, ParsedManpage>,
    /// lookup key -> list of (dst source, score)
    pub mappings: HashMap<String, Vec<(String, i32)>>,
}

/// Current bundle schema version written by export_wasm_data.py.
pub const BUNDLE_VERSION: u32 = 1;

/// In-memory manpage lookup index.
#[derive(Debug, Clone)]
pub struct ManpageData {
    bundle: DataBundle,
}

impl ManpageData {
    /// Load a bundle from MessagePack bytes.
    pub fn from_msgpack(bytes: &[u8]) -> Result<Self, DataError> {
        let bundle: DataBundle = rmp_serde::from_slice(bytes)?;
        if bundle.version != BUNDLE_VERSION {
            return Err(DataError::Decode(rmp_serde::decode::Error::Syntax(
                format!(
                    "unsupported bundle version {}, expected {}",
                    bundle.version, BUNDLE_VERSION
                ),
            )));
        }
        Ok(Self { bundle })
    }

    /// Number of manpages in the bundle.
    pub fn manpage_count(&self) -> usize {
        self.bundle.manpages.len()
    }

    /// Find the best manpage for a command name, with remaining suggestions.
    ///
    /// Mirrors Store.find_man_page: exact mapping wins, candidates sorted by
    /// score descending, only the top candidate is fully populated (all rows
    /// here are fully populated, so suggestions carry full data).
    pub fn find_man_page(
        &self,
        name: &str,
    ) -> Result<(&ParsedManpage, Vec<&ParsedManpage>), DataError> {
        // Direct source lookup (names ending in .gz in the Python version).
        if name.ends_with(".gz") {
            return self
                .bundle
                .manpages
                .get(name)
                .map(|m| (m, Vec::new()))
                .ok_or_else(|| DataError::ProgramNotFound(name.to_string()));
        }

        let mut candidates: Vec<(&ParsedManpage, i32)> = Vec::new();
        if let Some(dst_list) = self.bundle.mappings.get(name) {
            for (dst, score) in dst_list {
                if let Some(mp) = self.bundle.manpages.get(dst) {
                    candidates.push((mp, *score));
                }
            }
        }

        // Dotted fallback: strip the last .section suffix and retry once,
        // e.g. systemd.exec -> systemd (Python Store behavior).
        if candidates.is_empty() && name != "." {
            if let Some((head, _tail)) = name.rsplit_once('.') {
                if let Some(dst_list) = self.bundle.mappings.get(head) {
                    for (dst, score) in dst_list {
                        if let Some(mp) = self.bundle.manpages.get(dst) {
                            candidates.push((mp, *score));
                        }
                    }
                }
            }
        }

        if candidates.is_empty() {
            return Err(DataError::ProgramNotFound(name.to_string()));
        }

        candidates.sort_by(|a, b| b.1.cmp(&a.1));
        let top = candidates[0].0;
        let suggestions = candidates[1..].iter().map(|(m, _)| *m).collect();
        Ok((top, suggestions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use explainshell_core::{CliOption as EsOption, ExtractionMeta, NestedCmd};

    fn sample_bundle() -> DataBundle {
        let mp = ParsedManpage {
            source: "ubuntu/26.04/1/tar.1.gz".to_string(),
            name: "tar".to_string(),
            synopsis: Some("manipulate tape archives".to_string()),
            options: vec![EsOption {
                text: "verbose".to_string(),
                short: vec!["-v".to_string()],
                long: vec!["--verbose".to_string()],
                has_argument: explainshell_core::HasArgument::Bool(false),
                positional: None,
                prefix: None,
                nested_cmd: NestedCmd::Bool(false),
                meta: None,
            }],
            aliases: vec![("tar".to_string(), 10)],
            dashless_opts: false,
            subcommands: vec![],
            updated: false,
            nested_cmd: NestedCmd::Bool(false),
            extractor: Some("llm".to_string()),
            extraction_meta: Some(ExtractionMeta {
                model: Some("openai/gpt-5-mini".to_string()),
            }),
        };
        let mut manpages = HashMap::new();
        manpages.insert(mp.source.clone(), mp);
        let mut mappings = HashMap::new();
        mappings.insert(
            "tar".to_string(),
            vec![("ubuntu/26.04/1/tar.1.gz".to_string(), 10)],
        );
        DataBundle {
            version: BUNDLE_VERSION,
            manpages,
            mappings,
        }
    }

    #[test]
    fn roundtrip_msgpack() {
        let bundle = sample_bundle();
        let bytes = rmp_serde::to_vec(&bundle).expect("encode");
        let data = ManpageData::from_msgpack(&bytes).expect("decode");
        assert_eq!(data.manpage_count(), 1);
        let (top, suggestions) = data.find_man_page("tar").expect("lookup");
        assert_eq!(top.name, "tar");
        assert!(suggestions.is_empty());
    }

    #[test]
    fn unknown_program_errors() {
        let bundle = sample_bundle();
        let bytes = rmp_serde::to_vec(&bundle).expect("encode");
        let data = ManpageData::from_msgpack(&bytes).expect("decode");
        let err = data.find_man_page("no-such-cmd").unwrap_err();
        assert!(matches!(err, DataError::ProgramNotFound(_)));
    }

    /// Decode the real export output produced by scripts/export_wasm_data.py
    /// from the scripts/make_test_db.py fixture database.
    const FIXTURE_EXPORT: &[u8] =
        include_bytes!("../../../test/fixtures/test.data.msgpack");

    #[test]
    fn decodes_exported_fixture() {
        let data = ManpageData::from_msgpack(FIXTURE_EXPORT).expect("decode");
        assert_eq!(data.manpage_count(), 4);

        let (tar, _) = data.find_man_page("tar").expect("tar lookup");
        assert_eq!(tar.name, "tar");
        assert!(tar.find_option("--verbose").is_some());
        assert!(tar.find_option("-f").is_some());

        // Subcommand mapping: "git commit" resolves to git-commit.
        let (commit, _) = data.find_man_page("git commit").expect("subcommand");
        assert_eq!(commit.name, "git-commit");

        // Nested command flag survives the round trip.
        let (sudo, _) = data.find_man_page("sudo").expect("sudo lookup");
        assert_eq!(sudo.nested_cmd, NestedCmd::Bool(true));
    }
}
