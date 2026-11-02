use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub name: String,
    pub publisher: String,
    pub version: String,
    pub display_name: String,
    pub description: String,
    pub license: String,
    pub repository: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub echobird: Extension,
    #[serde(flatten)]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Extension {
    pub manifest_version: u32,
    pub api_version: String,
    pub platforms: Vec<String>,
    pub permissions: Vec<String>,
    pub contributes: Contributions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker: Option<Worker>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Worker {
    pub entry: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Contributions {
    pub pages: Vec<Page>,
    #[serde(default)]
    pub providers: Vec<ProviderContribution>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub themes: Vec<super::product::Theme>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub configuration: Vec<super::product::Configuration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<super::product::Command>,
    #[serde(
        default,
        rename = "textProcessors",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub text_processors: Vec<super::product::TextProcessor>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderContribution {
    pub id: String,
    pub title: String,
    pub applications: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_id: Option<String>,
    #[serde(default)]
    pub search_terms: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operations: Vec<String>,
}

/// An application a plugin adds to the catalog. The directory holds the same
/// `paths.json` and `config.json` as a built-in tool, plus `install.json`.
/// Declared at the manifest root so hosts without this contract ignore it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ToolContribution {
    pub id: String,
    pub directory: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub id: String,
    pub title: String,
    pub entry: String,
}

pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

pub fn relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 240
        && value.split('/').all(|part| {
            let stem = part
                .split('.')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with(['.', ' '])
                && !part
                    .chars()
                    .any(|c| c.is_control() || c.is_whitespace() || "\\:<>\"|?*%#".contains(c))
                && !["con", "prn", "aux", "nul"].contains(&stem.as_str())
                && !(stem.len() == 4
                    && (stem.starts_with("com") || stem.starts_with("lpt"))
                    && stem.as_bytes()[3].is_ascii_digit())
        })
}

pub fn public_https(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.host_str().is_some_and(|host| {
                host.contains('.')
                    && !host.ends_with(".localhost")
                    && !host.ends_with(".local")
                    && host.parse::<std::net::IpAddr>().is_err()
            })
    })
}

pub fn platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

impl Manifest {
    pub fn id(&self) -> String {
        format!("{}.{}", self.publisher, self.name)
    }

    pub fn tools(&self) -> Vec<ToolContribution> {
        self.metadata
            .get("tools")
            .and_then(|tools| serde_json::from_value(tools.clone()).ok())
            .unwrap_or_default()
    }

    fn validate_tools(&self) -> Result<(), String> {
        let Some(declared) = self.metadata.get("tools") else {
            return Ok(());
        };
        let tools: Vec<ToolContribution> =
            serde_json::from_value(declared.clone()).map_err(|_| "Invalid tool contribution")?;
        let extension = &self.echobird;
        let mut ids = std::collections::HashSet::new();
        let service = extension
            .worker
            .as_ref()
            .is_some_and(|worker| worker.runtime.as_deref() == Some("node"));
        if tools.is_empty()
            || tools.len() > 4
            || !service
            || tools.iter().any(|tool| {
                !identifier(&tool.id)
                    || !ids.insert(&tool.id)
                    || !relative_path(&tool.directory)
                    || !extension
                        .contributes
                        .providers
                        .iter()
                        .any(|provider| provider.applications.contains(&tool.id))
            })
        {
            return Err("Invalid tool contribution".into());
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if !identifier(&self.name)
            || !identifier(&self.publisher)
            || semver::Version::parse(&self.version).is_err()
            || self.display_name.trim().is_empty()
            || self.display_name.chars().count() > 80
            || self.description.trim().is_empty()
            || self.description.chars().count() > 240
            || self.license.trim().is_empty()
            || self.license.len() > 100
            || !public_https(&self.repository)
        {
            return Err("Invalid plugin metadata".into());
        }
        let extension = &self.echobird;
        if extension.manifest_version != 1 || extension.api_version != "0.1-draft" {
            return Err("Unsupported plugin API version".into());
        }
        let unique = |items: &[String]| {
            items.iter().collect::<std::collections::HashSet<_>>().len() == items.len()
        };
        if extension.platforms.is_empty()
            || !unique(&extension.platforms)
            || extension
                .platforms
                .iter()
                .any(|p| !["windows", "macos", "linux"].contains(&p.as_str()))
            || !extension.platforms.iter().any(|p| p == platform())
        {
            return Err("Plugin does not support this platform".into());
        }
        if !unique(&extension.permissions)
            || extension.permissions.iter().any(|p| {
                !["model.request", "clipboard.write", "account.code"].contains(&p.as_str())
                    && !super::product::PERMISSIONS.contains(&p.as_str())
            })
        {
            return Err("Unsupported plugin permission".into());
        }
        let pages = &extension.contributes.pages;
        super::product::validate(extension)?;
        let mut ids = std::collections::HashSet::new();
        if (pages.is_empty()
            && extension.worker.is_none()
            && extension.background.is_none()
            && extension.contributes.themes.is_empty())
            || pages.len() > 16
            || pages.iter().any(|page| {
                !identifier(&page.id)
                    || !ids.insert(&page.id)
                    || page.title.trim().is_empty()
                    || page.title.chars().count() > 80
                    || !relative_path(&page.entry)
                    || !page.entry.ends_with(".html")
            })
            || self.icon.as_ref().is_some_and(|p| !relative_path(p))
        {
            return Err("Invalid plugin page or icon".into());
        }
        let providers = &extension.contributes.providers;
        let mut provider_ids = std::collections::HashSet::new();
        if providers.len() > 16
            || providers.iter().any(|provider| {
                !identifier(&provider.id)
                    || !provider_ids.insert(&provider.id)
                    || provider.title.trim().is_empty()
                    || provider.title.chars().count() > 80
                    || provider.page_id.as_ref().is_some_and(|id| {
                        !identifier(id) || !pages.iter().any(|page| &page.id == id)
                    })
                    || (provider.page_id.is_none() && provider.operations.is_empty())
                    || provider.applications.is_empty()
                    || provider.applications.len() > 16
                    || !unique(&provider.applications)
                    || provider.applications.iter().any(|id| !identifier(id))
                    || provider.search_terms.len() > 32
                    || !unique(&provider.search_terms)
                    || provider
                        .search_terms
                        .iter()
                        .any(|term| term.trim().is_empty() || term.chars().count() > 80)
                    || !unique(&provider.operations)
                    || provider.operations.iter().any(|operation| {
                        ![
                            "accounts",
                            "login.start",
                            "login.complete",
                            "login.poll",
                            "login.cancel",
                            "account.refresh",
                            "account.warmup",
                            "account.claim",
                            "account.apply",
                            "account.remove",
                            "models",
                            "proxy",
                            "voice",
                            "history.merge",
                        ]
                        .contains(&operation.as_str())
                    })
                    || (!provider.operations.is_empty() && extension.worker.is_none())
            })
        {
            return Err("Invalid provider contribution".into());
        }
        if let Some(worker) = &extension.worker {
            if !relative_path(&worker.entry)
                || match worker.runtime.as_deref() {
                    None | Some("node") => !worker.entry.ends_with(".mjs"),
                    Some("native") => false,
                    Some(_) => true,
                }
                || !extension.permissions.iter().any(|p| p == "account.code")
                || providers
                    .iter()
                    .all(|provider| provider.operations.is_empty())
                || (worker.runtime.as_deref() == Some("native")
                    && (providers.len() != 1 || providers[0].applications.len() != 1))
            {
                return Err("Invalid account code module".into());
            }
        }
        self.validate_tools()
    }

    pub fn read(root: &Path) -> Result<Self, String> {
        let bytes =
            std::fs::read(root.join("package.json")).map_err(|_| "package.json is missing")?;
        if bytes.len() > 128 * 1024 {
            return Err("Plugin manifest is too large".into());
        }
        let manifest: Self =
            serde_json::from_slice(&bytes).map_err(|e| format!("Invalid package.json: {e}"))?;
        manifest.validate()?;
        if let Some(entry) = &manifest.echobird.background {
            let package = root.canonicalize().map_err(|e| e.to_string())?;
            let file = root
                .join(entry)
                .canonicalize()
                .map_err(|_| "Missing background entry")?;
            if !file.starts_with(&package) || !file.is_file() {
                return Err("Background entry escapes its package".into());
            }
        }
        if let Some(worker) = &manifest.echobird.worker {
            let entry = root
                .join(&worker.entry)
                .canonicalize()
                .map_err(|_| "Missing account code module")?;
            let package = root.canonicalize().map_err(|e| e.to_string())?;
            if !entry.starts_with(package) || !entry.is_file() {
                return Err("Account code module escapes its package".into());
            }
        }
        for tool in manifest.tools() {
            for file in ["paths.json", "config.json", "install.json"] {
                if !root.join(&tool.directory).join(file).is_file() {
                    return Err(format!("Missing tool file: {}/{file}", tool.directory));
                }
            }
        }
        for page in &manifest.echobird.contributes.pages {
            if !root.join(&page.entry).is_file() {
                return Err(format!("Missing page: {}", page.entry));
            }
        }
        Ok(manifest)
    }
}
