use crate::config::TargetConfig;
use rsemu_core::TargetSpec;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct TargetRegistry {
    configs: HashMap<String, TargetConfig>,
    svds: HashMap<String, String>,
    svds_dir: PathBuf,
}

impl TargetRegistry {
    /// Scan configs_dir for *.json files and build the registry.
    /// SVD files are read from svds_dir on demand.
    pub fn from_dirs(configs_dir: &Path, svds_dir: &Path) -> Result<Self, String> {
        let mut configs = HashMap::new();
        let entries = fs::read_dir(configs_dir)
            .map_err(|e| format!("cannot read configs dir '{}': {e}", configs_dir.display()))?;

        for entry in entries {
            let entry = entry.map_err(|e| format!("read dir entry: {e}"))?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let raw = fs::read_to_string(&path)
                .map_err(|e| format!("read '{}': {e}", path.display()))?;
            let config: TargetConfig = serde_json::from_str(&raw)
                .map_err(|e| format!("parse '{}': {e}", path.display()))?;
            configs.insert(config.id.clone(), config);
        }

        Ok(Self {
            configs,
            svds: HashMap::new(),
            svds_dir: svds_dir.to_path_buf(),
        })
    }

    /// Create registry from compile-time embedded (json, svd) string pairs.
    /// Used by GUI apps that need zero runtime file access.
    pub fn from_embedded(pairs: Vec<(&str, &str)>) -> Result<Self, String> {
        let mut configs = HashMap::new();
        let mut svds = HashMap::new();

        for (json_str, svd_str) in pairs {
            let config: TargetConfig = serde_json::from_str(json_str)
                .map_err(|e| format!("parse embedded config: {e}"))?;
            svds.insert(config.svd_file.clone(), svd_str.to_string());
            configs.insert(config.id.clone(), config);
        }

        Ok(Self {
            configs,
            svds,
            svds_dir: PathBuf::new(),
        })
    }

    /// List all available target IDs.
    pub fn list_ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.configs.keys().map(|s| s.as_str()).collect();
        ids.sort();
        ids
    }

    /// Load a TargetSpec by ID — parses SVD, applies patches.
    pub fn load(&self, id: &str) -> Result<TargetSpec, String> {
        let config = self
            .configs
            .get(id)
            .ok_or_else(|| format!("unknown target '{id}'"))?;

        let svd_xml = if let Some(embedded) = self.svds.get(&config.svd_file) {
            embedded.clone()
        } else {
            let svd_path = self.svds_dir.join(&config.svd_file);
            fs::read_to_string(&svd_path)
                .map_err(|e| format!("read SVD '{}': {e}", svd_path.display()))?
        };

        config.build_target_spec(&svd_xml)
    }

    /// Get a reference to the raw config (for board info display, etc.).
    pub fn get_config(&self, id: &str) -> Option<&TargetConfig> {
        self.configs.get(id)
    }
}
