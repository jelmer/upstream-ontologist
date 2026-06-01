use crate::{
    Certainty, GuesserSettings, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata,
    UpstreamMetadata,
};
use serde::Deserialize;
use std::collections::HashMap;

/// Extracts upstream metadata from Cargo.toml file
#[cfg(feature = "cargo")]
pub fn guess_from_cargo(
    path: &std::path::Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    // see https://doc.rust-lang.org/cargo/reference/manifest.html
    let mut manifest = cargo_toml::Manifest::from_path(path)
        .map_err(|e| ProviderError::ParseError(e.to_string()))?;

    // Try to resolve workspace inheritance from a parent workspace
    if manifest.needs_workspace_inheritance() {
        if let Some(workspace_path) = path.parent().and_then(|p| p.parent()) {
            let workspace_toml = workspace_path.join("Cargo.toml");
            if workspace_toml.exists() {
                if let Ok(workspace_manifest) = cargo_toml::Manifest::from_path(&workspace_toml) {
                    let _ = manifest.complete_from_path_and_workspace(
                        path,
                        Some((&workspace_manifest, workspace_path)),
                    );
                }
            }
        }
    }

    let package = match manifest.package {
        Some(ref p) => p,
        None => {
            log::debug!("No package section in Cargo.toml");
            return Ok(Vec::new());
        }
    };

    let mut results = Vec::new();

    let name = &package.name;
    results.push(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name(name.clone()),
        certainty: Some(Certainty::Certain),
        origin: Some(path.into()),
    });
    results.push(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::CargoCrate(name.clone()),
        certainty: Some(Certainty::Certain),
        origin: Some(path.into()),
    });

    if let Some(description) = package.description() {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Summary(description.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(homepage) = package.homepage() {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Homepage(homepage.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(license) = package.license() {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::License(license.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(repository) = package.repository() {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository(repository.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    let version = package.version();
    results.push(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Version(version.to_string()),
        certainty: Some(Certainty::Certain),
        origin: Some(path.into()),
    });

    let authors = package.authors();
    if !authors.is_empty() {
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
    let url: url::Url = format!("https://crates.io/api/v1/crates?q={}", crate_name)
        .parse()
        .expect("static crates.io URL template should always be valid");
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

/// Category information from crates.io
#[derive(Deserialize)]
pub struct Category {
    /// Category identifier
    pub id: String,
    /// Category display name
    pub category: String,
}

/// Keyword information from crates.io
#[derive(Deserialize)]
pub struct Keyword {
    /// Keyword identifier
    pub id: String,
    /// Keyword text
    pub keyword: String,
}

/// Information about a crate from crates.io
#[derive(Deserialize)]
pub struct CrateInfo {
    /// Categories the crate belongs to
    pub categories: Vec<Category>,
    #[serde(rename = "crate")]
    crate_: Crate,
    /// Keywords associated with the crate
    pub keywords: Vec<Keyword>,
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
    let http_url: url::Url = format!("https://crates.io/api/v1/crates/{}", cratename)
        .parse()
        .map_err(|e: url::ParseError| crate::ProviderError::Other(e.to_string()))?;

    let data = crate::load_json_url(&http_url, None).await?;

    serde_json::from_value(data)
        .map(Some)
        .map_err(|e| crate::ProviderError::ParseError(format!("Failed to parse crate data: {}", e)))
}

/// A dependency of a crate version, as recorded in the registry index.
///
/// See <https://doc.rust-lang.org/cargo/reference/registry-index.html>.
#[derive(Deserialize, Debug, Clone)]
pub struct IndexDependency {
    /// Name of the dependency (the renamed name if it was renamed).
    pub name: String,
    /// SemVer requirement for this dependency.
    pub req: String,
    /// Features enabled for this dependency.
    #[serde(default)]
    pub features: Vec<String>,
    /// Whether this is an optional dependency.
    #[serde(default)]
    pub optional: bool,
    /// Whether the dependency's default features are enabled.
    #[serde(default = "default_true")]
    pub default_features: bool,
    /// Target platform expression this dependency applies to, if any.
    #[serde(default)]
    pub target: Option<String>,
    /// Dependency kind: `normal`, `dev` or `build`.
    #[serde(default)]
    pub kind: Option<String>,
    /// Registry index URL, or `None` for the current registry.
    #[serde(default)]
    pub registry: Option<String>,
    /// Original package name if the dependency was renamed.
    #[serde(default)]
    pub package: Option<String>,
}

fn default_true() -> bool {
    true
}

/// A single crate version entry from the crates.io sparse index.
///
/// The sparse index only carries the data needed for dependency resolution
/// (name, version, dependencies, features, checksum). It does not include the
/// rich metadata served by the crates.io REST API such as the homepage,
/// repository, description or license; use [`load_crate_info`] for those.
#[derive(Deserialize, Debug, Clone)]
pub struct IndexEntry {
    /// Name of the crate.
    pub name: String,
    /// Version of this entry.
    pub vers: semver::Version,
    /// Dependencies of this version.
    pub deps: Vec<IndexDependency>,
    /// SHA256 checksum of the `.crate` file.
    pub cksum: String,
    /// Map of feature names to the features or dependencies they enable.
    #[serde(default)]
    pub features: HashMap<String, Vec<String>>,
    /// Map of features using the 1.60+ extended syntax, if present.
    #[serde(default)]
    pub features2: Option<HashMap<String, Vec<String>>>,
    /// Whether this version has been yanked.
    #[serde(default)]
    pub yanked: bool,
    /// The `links` value from the manifest, if any.
    #[serde(default)]
    pub links: Option<String>,
    /// Schema version of this entry, defaulting to 1.
    #[serde(default)]
    pub v: Option<u32>,
    /// Minimum supported Rust version, if declared.
    #[serde(default)]
    pub rust_version: Option<String>,
}

/// Computes the registry index path for a crate name, per RFC 2789.
///
/// The same layout is used by the sparse index and the `crates.io-index` git
/// mirror. Names are lower-cased and bucketed by length: 1-3 character names
/// use dedicated prefixes, longer names are split into two-character
/// directories.
fn index_path(name: &str) -> String {
    let lower = name.to_lowercase();
    match lower.len() {
        1 => format!("1/{}", lower),
        2 => format!("2/{}", lower),
        3 => format!("3/{}/{}", &lower[..1], lower),
        _ => format!("{}/{}/{}", &lower[..2], &lower[2..4], lower),
    }
}

/// Loads all version entries for a crate from the crates.io registry index.
///
/// The index data is fetched from the `rust-lang/crates.io-index` repository on
/// GitHub, which mirrors the same per-crate metadata as the sparse index at
/// [`index.crates.io`](https://index.crates.io/).
///
/// Returns `None` if the crate is not present in the index. The entries are
/// returned in the order they appear in the index, which is publication order
/// (oldest first).
pub async fn load_index_info(
    cratename: &str,
) -> Result<Option<Vec<IndexEntry>>, crate::ProviderError> {
    let body = match crate::github::download_raw_file(
        "rust-lang",
        "crates.io-index",
        "master",
        &index_path(cratename),
    )
    .await
    {
        Ok(body) => body,
        Err(crate::HTTPJSONError::Error { status: 404, .. }) => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    let mut entries = Vec::new();
    for line in body.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: IndexEntry = serde_json::from_str(line).map_err(|e| {
            crate::ProviderError::ParseError(format!("Failed to parse index entry: {}", e))
        })?;
        entries.push(entry);
    }

    Ok(Some(entries))
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

    #[test]
    fn test_index_path() {
        assert_eq!(index_path("a"), "1/a");
        assert_eq!(index_path("ab"), "2/ab");
        assert_eq!(index_path("abc"), "3/a/abc");
        assert_eq!(index_path("serde"), "se/rd/serde");
        assert_eq!(index_path("Inflector"), "in/fl/inflector");
        assert_eq!(index_path("cargo-edit"), "ca/rg/cargo-edit");
    }

    #[test]
    fn test_parse_index_entry() {
        let line = r#"{"name":"serde","vers":"1.0.0","deps":[{"name":"serde_derive","req":"^1.0","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal"}],"cksum":"00000000000000000000000000000000000000000000000000000000000000aa","features":{"derive":["serde_derive"],"default":["std"]},"yanked":false}"#;

        let entry: IndexEntry = serde_json::from_str(line).unwrap();

        assert_eq!(entry.name, "serde");
        assert_eq!(entry.vers, semver::Version::new(1, 0, 0));
        assert_eq!(entry.deps.len(), 1);
        assert_eq!(entry.deps[0].name, "serde_derive");
        assert!(entry.deps[0].optional);
        assert!(!entry.yanked);
    }
}
