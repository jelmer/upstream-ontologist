use crate::{
    Certainty, GuesserSettings, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata,
    UpstreamMetadata,
};
use serde::Deserialize;
use std::collections::HashMap;

#[cfg(feature = "cargo")]
#[derive(Deserialize)]
struct CargoToml {
    package: Option<CargoPackage>,

    workspace: Option<CargoWorkspace>,
}

#[cfg(feature = "cargo")]
#[derive(Deserialize)]
struct CargoWorkspace {
    #[serde(default)]
    package: Option<CargoPackage>,
}

#[cfg(feature = "cargo")]
/// Allow either specifying setting T directly or "workspace = true"
pub enum DirectOrWorkspace<T> {
    /// Direct value specification
    Direct(T),
    /// Workspace inheritance
    Workspace,
}

#[cfg(feature = "cargo")]
impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for DirectOrWorkspace<T> {
    fn deserialize<D>(deserializer: D) -> Result<DirectOrWorkspace<T>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Assume deserializing T, but if that fails, check for a table with "workspace = true"
        let v: toml::value::Value = serde::Deserialize::deserialize(deserializer)?;
        match T::deserialize(v.clone()) {
            Ok(t) => Ok(DirectOrWorkspace::Direct(t)),
            Err(_) => {
                let table = v.as_table().ok_or_else(|| {
                    serde::de::Error::custom("expected either a value or a table")
                })?;
                if table.get("workspace").and_then(|v| v.as_bool()) == Some(true) {
                    Ok(DirectOrWorkspace::Workspace)
                } else {
                    Err(serde::de::Error::custom(
                        "expected either a value or a table",
                    ))
                }
            }
        }
    }
}

#[cfg(feature = "cargo")]
#[derive(Deserialize)]
struct CargoPackage {
    name: Option<String>,
    #[serde(default)]
    version: Option<DirectOrWorkspace<String>>,
    #[serde(default)]
    authors: Option<Vec<String>>,
    #[serde(default)]
    description: Option<DirectOrWorkspace<String>>,
    #[serde(default)]
    homepage: Option<DirectOrWorkspace<String>>,
    #[serde(default)]
    repository: Option<DirectOrWorkspace<String>>,
    #[serde(default)]
    license: Option<DirectOrWorkspace<String>>,
}

#[cfg(feature = "cargo")]
macro_rules! resolve {
    ($workspace:expr, $package:expr, $field:ident) => {
        match $package.$field {
            Some(DirectOrWorkspace::Direct(ref s)) => Some(s.clone()),
            Some(DirectOrWorkspace::Workspace) => {
                if let Some(DirectOrWorkspace::Direct(ref s)) =
                    $workspace.package.as_ref().and_then(|p| p.$field.as_ref())
                {
                    Some(s.clone())
                } else {
                    None
                }
            }
            None => None,
        }
    };
}

/// Extracts upstream metadata from Cargo.toml file
#[cfg(feature = "cargo")]
pub fn guess_from_cargo(
    path: &std::path::Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    // see https://doc.rust-lang.org/cargo/reference/manifest.html
    let doc: CargoToml = toml::from_str(&std::fs::read_to_string(path)?)
        .map_err(|e| ProviderError::ParseError(e.to_string()))?;

    let package = match doc.package {
        Some(p) => p,
        None => {
            log::debug!("No package section in Cargo.toml");
            return Ok(Vec::new());
        }
    };

    let workspace = match doc.workspace {
        Some(w) => w,
        None => CargoWorkspace { package: None },
    };

    let mut results = Vec::new();

    if let Some(name) = package.name {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(name.clone()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::CargoCrate(name),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(description) = resolve!(workspace, package, description) {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Summary(description),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(homepage) = resolve!(workspace, package, homepage) {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Homepage(homepage),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(license) = resolve!(workspace, package, license) {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::License(license),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(repository) = resolve!(workspace, package, repository) {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository(repository),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(version) = resolve!(workspace, package, version) {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Version(version),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(authors) = package.authors {
        let authors = authors.iter().map(|a| Person::from(a.as_str())).collect();
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Author(authors),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    Ok(results)
}

/// Translates crate names with dashes to their canonical form on crates.io
pub async fn cargo_translate_dashes(
    crate_name: &str,
) -> Result<Option<String>, crate::HTTPJSONError> {
    let url = format!("https://crates.io/api/v1/crates?q={}", crate_name)
        .parse()
        .unwrap();
    let json: serde_json::Value = crate::load_json_url(&url, None).await?;

    // Navigate through the JSON response to find the crate name.
    if let Some(crates) = json.get("crates").and_then(|c| c.as_array()) {
        for krate in crates {
            if let Some(name) = krate.get("id").and_then(|n| n.as_str()) {
                return Ok(Some(name.to_string()));
            }
        }
    }

    Ok(None)
}

/// Crate metadata from crates.io
#[derive(Deserialize)]
pub struct Crate {
    /// Crate badges
    pub badges: Vec<String>,
    /// Creation timestamp
    pub created_at: String,
    /// Crate description
    pub description: Option<String>,
    /// Documentation URL
    pub documentation: Option<String>,
    /// Total downloads
    pub downloads: i64,
    /// Homepage URL
    pub homepage: Option<String>,
    /// Crate identifier
    pub id: String,
    /// Keywords
    pub keywords: Vec<String>,
    /// License identifier
    pub license: Option<String>,
    /// Various links
    pub links: HashMap<String, Option<String>>,
    /// Maximum stable version
    pub max_stable_version: semver::Version,
    /// Maximum version
    pub max_version: semver::Version,
    /// Crate name
    pub name: String,
    /// Newest version
    pub newest_version: semver::Version,
    /// Recent downloads
    pub recent_downloads: i64,
    /// Repository URL
    pub repository: Option<String>,
    /// Last update timestamp
    pub updated_at: String,
    /// Version IDs
    pub versions: Option<Vec<i32>>,
}

/// User information from crates.io
#[derive(Deserialize)]
pub struct User {
    /// User avatar URL
    pub avatar: String,
    /// User ID
    pub id: i32,
    /// User login name
    pub login: String,
    /// User display name
    pub name: String,
    /// User profile URL
    pub url: String,
}

/// Audit action information
#[derive(Deserialize)]
pub struct AuditAction {
    /// Action type
    pub action: String,
    /// Timestamp of the action
    pub time: String,
    /// User who performed the action
    pub user: User,
}

/// Information about a specific version of a crate
#[derive(Deserialize)]
pub struct CrateVersion {
    /// Audit actions for this version
    pub audit_actions: Vec<AuditAction>,
    /// Names of binary targets
    pub bin_names: Vec<String>,
    /// Checksum of the crate
    pub checksum: String,
    /// Name of the crate
    #[serde(rename = "crate")]
    pub crate_: String,
    /// Size of the crate in bytes
    pub crate_size: i64,
    /// Creation timestamp
    pub created_at: String,
    /// Download path
    pub dl_path: String,
    /// Number of downloads
    pub downloads: i64,
    /// Feature flags
    pub features: HashMap<String, Vec<String>>,
    /// Whether the crate has a library
    pub has_lib: bool,
    /// Version ID
    pub id: i32,
    /// Library links
    pub lib_links: Option<HashMap<String, String>>,
    /// License identifier
    pub license: Option<String>,
    /// Various links
    pub links: HashMap<String, Option<String>>,
    /// Version number
    pub num: semver::Version,
    /// User who published this version
    pub published_by: Option<User>,
    /// Path to README file
    pub readme_path: String,
    /// Minimum Rust version required
    pub rust_version: Option<String>,
    /// Last update timestamp
    pub updated_at: String,
    /// Whether this version is yanked
    pub yanked: bool,
}

/// Information about a crate from crates.io
#[derive(Deserialize)]
pub struct CrateInfo {
    /// Categories the crate belongs to
    pub categories: Vec<String>,
    #[serde(rename = "crate")]
    crate_: Crate,
    /// Keywords associated with the crate
    pub keywords: Vec<String>,
    /// All versions of the crate
    pub versions: Vec<CrateVersion>,
}

impl TryFrom<CrateInfo> for UpstreamMetadata {
    type Error = crate::ProviderError;

    fn try_from(value: CrateInfo) -> Result<Self, Self::Error> {
        let mut ret = UpstreamMetadata::default();

        ret.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(value.crate_.name.to_string()),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        if let Some(homepage) = value.crate_.homepage {
            ret.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Homepage(homepage),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        if let Some(repository) = value.crate_.repository {
            ret.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(repository),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        if let Some(description) = value.crate_.description {
            ret.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Summary(description),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        if let Some(license) = value.crate_.license {
            ret.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::License(license),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        ret.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Version(value.crate_.newest_version.to_string()),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        Ok(ret)
    }
}

/// Loads crate information from crates.io API
pub async fn load_crate_info(cratename: &str) -> Result<Option<CrateInfo>, crate::ProviderError> {
    let http_url = format!("https://crates.io/api/v1/crates/{}", cratename);

    let data = crate::load_json_url(&http_url.parse().unwrap(), None).await?;

    Ok(Some(serde_json::from_value(data).unwrap()))
}

// TODO: dedupe with TryFrom implementation above
fn parse_crates_io(data: &CrateInfo) -> Vec<UpstreamDatum> {
    let crate_data = &data.crate_;
    let mut results = Vec::new();
    results.push(UpstreamDatum::Name(crate_data.name.to_string()));
    if let Some(homepage) = crate_data.homepage.as_ref() {
        results.push(UpstreamDatum::Homepage(homepage.to_string()));
    }
    if let Some(repository) = crate_data.repository.as_ref() {
        results.push(UpstreamDatum::Repository(repository.to_string()));
    }
    if let Some(description) = crate_data.description.as_ref() {
        results.push(UpstreamDatum::Summary(description.to_string()));
    }
    if let Some(license) = crate_data.license.as_ref() {
        results.push(UpstreamDatum::License(license.to_string()));
    }
    results.push(UpstreamDatum::Version(
        crate_data.newest_version.to_string(),
    ));

    results
}

/// Crates.io metadata provider
pub struct CratesIo;

impl Default for CratesIo {
    fn default() -> Self {
        Self::new()
    }
}

impl CratesIo {
    /// Creates a new CratesIo provider
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl crate::ThirdPartyRepository for CratesIo {
    fn name(&self) -> &'static str {
        "crates.io"
    }

    fn max_supported_certainty(&self) -> Certainty {
        Certainty::Certain
    }

    fn supported_fields(&self) -> &'static [&'static str] {
        &["Homepage", "Name", "Repository", "Version", "Summary"][..]
    }

    async fn guess_metadata(&self, name: &str) -> Result<Vec<UpstreamDatum>, ProviderError> {
        let data = load_crate_info(name).await?;
        if data.is_none() {
            return Ok(Vec::new());
        }
        Ok(parse_crates_io(&data.unwrap()))
    }
}

/// Fetches upstream metadata for a crate from crates.io
pub async fn remote_crate_data(name: &str) -> Result<UpstreamMetadata, crate::ProviderError> {
    let data = load_crate_info(name).await?;

    if let Some(data) = data {
        Ok(data.try_into()?)
    } else {
        Ok(UpstreamMetadata::default())
    }
}

#[cfg(test)]
mod crates_io_tests {
    use super::*;

    #[test]
    fn test_load_crate_info() {
        let data = include_str!("../testdata/crates.io.json");

        let crate_info: CrateInfo = serde_json::from_str(data).unwrap();

        assert_eq!(crate_info.crate_.name, "breezy");
    }
}
