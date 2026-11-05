pub mod manifest;
mod packages;
pub mod product;
pub use packages::registry;
#[cfg(test)]
mod tests;

use manifest::Manifest;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub const INTERNATIONAL_INDEX: &str =
    "https://raw.githubusercontent.com/edison7009/echobird-plugin/main/index.json";
pub const CHINA_INDEX: &str =
    "https://api.gitcode.com/api/v5/repos/edison7009/echobird-plugin/raw/index.json";
pub const CNB_INDEX: &str = "https://cnb.cool/echobird/echobird-plugin/-/git/raw/main/index.json";
/// The CNB mirror holds the same files as GitCode. The index only lists GitCode
/// and GitHub addresses, so CNB serves the China entries from the same paths.
pub const GITCODE_RAW: &str =
    "https://api.gitcode.com/api/v5/repos/edison7009/echobird-plugin/raw/";
pub const CNB_RAW: &str = "https://cnb.cool/echobird/echobird-plugin/-/git/raw/main/";
pub const MAX_PACKAGE_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPlugin {
    pub id: String,
    pub manifest: Manifest,
    pub enabled: bool,
    pub directory: String,
    pub source: String,
    pub revision: Option<String>,
    pub icon: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub token: String,
    pub manifest: Manifest,
    pub source: String,
    pub revision: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryRelease {
    pub manifest: Manifest,
    pub commit: String,
    pub sha256: String,
    pub downloads: HashMap<String, String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RegistryPlugin {
    pub id: String,
    pub repository: String,
    #[serde(
        default,
        rename = "clientIcon",
        skip_serializing_if = "Option::is_none"
    )]
    pub client_icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descriptions: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icons: Option<HashMap<String, String>>,
    pub releases: Vec<RegistryRelease>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Registry {
    pub schema_version: u32,
    pub name: String,
    pub plugins: Vec<RegistryPlugin>,
}

impl Registry {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut registry: Self =
            serde_json::from_slice(bytes).map_err(|e| format!("Invalid plugin index: {e}"))?;
        if registry.schema_version != 1 || registry.name != "EchoBird Plugin Hub" {
            return Err("Unsupported plugin index".into());
        }
        let mut ids = std::collections::HashSet::new();
        for plugin in &mut registry.plugins {
            if !ids.insert(&plugin.id)
                || !manifest::public_https(&plugin.repository)
                || plugin.releases.is_empty()
                || plugin.descriptions.as_ref().is_some_and(|descriptions| {
                    descriptions.len() != 2
                        || ["international", "china"].iter().any(|region| {
                            !descriptions
                                .get(*region)
                                .is_some_and(|text| !text.is_empty() && text.chars().count() <= 500)
                        })
                })
                || plugin.client_icon.as_ref().is_some_and(|file| {
                    !file.rsplit_once('.').is_some_and(|(name, extension)| {
                        manifest::identifier(name) && matches!(extension, "svg" | "png")
                    })
                })
                || plugin.icons.as_ref().is_some_and(|icons| {
                    icons.len() != 2
                        || ["international", "china"].iter().any(|region| {
                            !icons
                                .get(*region)
                                .is_some_and(|url| manifest::public_https(url))
                        })
                })
            {
                return Err("Invalid registry plugin".into());
            }
            let mut versions = std::collections::HashSet::new();
            // A release this client cannot use is skipped, so a newer plugin capability
            // never takes the whole catalog away from people on an older client.
            plugin.releases.retain(|r| {
                r.manifest.validate().is_ok()
                    && r.manifest
                        .echobird
                        .platforms
                        .iter()
                        .any(|p| p == manifest::platform())
            });
            for release in &plugin.releases {
                let version =
                    semver::Version::parse(&release.manifest.version).map_err(|e| e.to_string())?;
                if release.manifest.id() != plugin.id
                    || release.manifest.repository != plugin.repository
                    || !versions.insert(version)
                    || !hex_value(&release.sha256, &[64])
                    || !hex_value(&release.commit, &[40, 64])
                    || release.downloads.len() != 2
                    || ["international", "china"].iter().any(|region| {
                        !release
                            .downloads
                            .get(*region)
                            .is_some_and(|url| manifest::public_https(url))
                    })
                {
                    return Err("Invalid registry release".into());
                }
            }
            plugin.releases.sort_by(|a, b| {
                semver::Version::parse(&b.manifest.version)
                    .unwrap()
                    .cmp(&semver::Version::parse(&a.manifest.version).unwrap())
            });
        }
        registry.plugins.retain(|p| !p.releases.is_empty());
        Ok(registry)
    }
}

fn hex_value(value: &str, lengths: &[usize]) -> bool {
    lengths.contains(&value.len())
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

pub struct PluginManager {
    pub root: PathBuf,
    mutation: Mutex<()>,
    candidates: Mutex<HashMap<String, Candidate>>,
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new(crate::utils::platform::echobird_dir().join("plugins"))
    }
}

impl PluginManager {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            mutation: Mutex::new(()),
            candidates: Mutex::new(HashMap::new()),
        }
    }

    pub fn list(&self) -> Result<Vec<InstalledPlugin>, String> {
        let path = self.root.join("installed.json");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let plugins: Vec<InstalledPlugin> =
            serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("Invalid plugin state: {e}"))?;
        let mut ids = std::collections::HashSet::new();
        for plugin in &plugins {
            plugin.manifest.validate()?;
            if plugin.id != plugin.manifest.id()
                || !ids.insert(&plugin.id)
                || !manifest::relative_path(&plugin.directory)
                || !plugin
                    .directory
                    .starts_with(&format!("packages/{}/", plugin.id))
            {
                return Err("Invalid installed plugin state".into());
            }
        }
        Ok(plugins)
    }

    fn save(&self, plugins: &[InstalledPlugin]) -> Result<(), String> {
        fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let temporary = self.root.join(format!("{}.json.tmp", uuid::Uuid::new_v4()));
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(plugins).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::rename(&temporary, self.root.join("installed.json")).map_err(|e| e.to_string())
    }

    pub fn package_root(&self, plugin: &InstalledPlugin) -> Result<PathBuf, String> {
        let root = self.root.canonicalize().map_err(|e| e.to_string())?;
        let package = root
            .join(&plugin.directory)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !package.starts_with(root.join("packages")) {
            return Err("Plugin path escapes storage".into());
        }
        Ok(package)
    }

    fn staging(&self, token: &str) -> PathBuf {
        self.root.join("staging").join(token)
    }

    pub fn cancel(&self, token: &str) -> Result<(), String> {
        if self.candidates.lock().unwrap().remove(token).is_some() {
            self.remove_owned(&self.staging(token))?;
        }
        Ok(())
    }

    pub fn pause(&self) {
        let _mutation = self.mutation.lock().unwrap();
        let tokens: Vec<_> = self
            .candidates
            .lock()
            .unwrap()
            .drain()
            .map(|(id, _)| id)
            .collect();
        for token in tokens {
            if let Err(error) = self.remove_owned(&self.staging(&token)) {
                log::warn!("Could not clear a paused plugin preview: {error}");
            }
        }
    }

    fn remove_owned(&self, path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let resolved = path.canonicalize().map_err(|e| e.to_string())?;
        let root = self.root.canonicalize().map_err(|e| e.to_string())?;
        if resolved == root
            || !(resolved.starts_with(root.join("staging"))
                || resolved.starts_with(root.join("packages")))
        {
            return Err("Refusing to remove unmanaged plugin path".into());
        }
        fs::remove_dir_all(resolved).map_err(|e| e.to_string())
    }

    pub fn install(&self, token: &str) -> Result<Vec<InstalledPlugin>, String> {
        let _lock = self.mutation.lock().unwrap();
        let candidate = self
            .candidates
            .lock()
            .unwrap()
            .get(token)
            .cloned()
            .ok_or("Plugin preview expired")?;
        let stage = self.staging(token).join("package");
        let manifest = Manifest::read(&stage)?;
        if manifest != candidate.manifest {
            return Err("Plugin changed after approval".into());
        }
        let mut plugins = self.list()?;
        let previous = plugins.iter().find(|p| p.id == manifest.id()).cloned();
        let directory = format!("packages/{}/{}", manifest.id(), uuid::Uuid::new_v4());
        let destination = self.root.join(&directory);
        fs::create_dir_all(destination.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::rename(&stage, &destination).map_err(|e| e.to_string())?;
        let plugin = InstalledPlugin {
            id: manifest.id(),
            icon: packages::icon(&destination, &manifest),
            manifest,
            enabled: previous.as_ref().is_none_or(|plugin| plugin.enabled),
            directory,
            source: candidate.source,
            revision: candidate.revision,
        };
        plugins.retain(|p| p.id != plugin.id);
        plugins.push(plugin);
        if let Err(error) = self.save(&plugins) {
            let _ = self.remove_owned(&destination);
            return Err(error);
        }
        self.candidates.lock().unwrap().remove(token);
        let _ = self.remove_owned(&self.staging(token));
        if let Some(old) = previous {
            let _ = self.remove_owned(&self.root.join(old.directory));
        }
        Ok(plugins)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<Vec<InstalledPlugin>, String> {
        let _lock = self.mutation.lock().unwrap();
        let mut plugins = self.list()?;
        plugins
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or("Plugin is not installed")?
            .enabled = enabled;
        self.save(&plugins)?;
        Ok(plugins)
    }

    pub fn uninstall(&self, id: &str) -> Result<Vec<InstalledPlugin>, String> {
        let _lock = self.mutation.lock().unwrap();
        let mut plugins = self.list()?;
        let old = plugins
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or("Plugin is not installed")?;
        plugins.retain(|p| p.id != id);
        self.save(&plugins)?;
        let _ = self.remove_owned(&self.root.join(old.directory));
        Ok(plugins)
    }
}
