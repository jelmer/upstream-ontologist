use crate::{
    Certainty, GuesserSettings, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata,
    UpstreamMetadata,
};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
struct CargoToml {
    package: Option<CargoPackage>,

    workspace: Option<CargoWorkspace>,
}

#[derive(Deserialize)]
struct CargoWorkspace {
    #[serde(default)]
    package: Option<CargoPackage>,
}

/// Allow either specifying setting T directly or "workspace = true"
pub enum DirectOrWorkspace<T> {
    Direct(T),
    Workspace,
}

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

#[derive(Deserialize)]
pub struct Crate {
    pub badges: Vec<String>,
    pub created_at: String,
    pub description: Option<String>,
    pub documentation: Option<String>,
    pub downloads: i64,
    pub homepage: Option<String>,
    pub id: String,
    pub keywords: Vec<String>,
    pub license: Option<String>,
    pub links: HashMap<String, Option<String>>,
    pub max_stable_version: semver::Version,
    pub max_version: semver::Version,
    pub name: String,
    pub newest_version: semver::Version,
    pub recent_downloads: i64,
    pub repository: Option<String>,
    pub updated_at: String,
    pub versions: Option<Vec<i32>>,
}

#[derive(Deserialize)]
pub struct User {
    pub avatar: String,
    pub id: i32,
    pub login: String,
    pub name: String,
    pub url: String,
}

#[derive(Deserialize)]
pub struct AuditAction {
    pub action: String,
    pub time: String,
    pub user: User,
}

#[derive(Deserialize)]
pub struct CrateVersion {
    pub audit_actions: Vec<AuditAction>,
    pub bin_names: Vec<String>,
    pub checksum: String,
    #[serde(rename = "crate")]
    pub crate_: String,
    pub crate_size: i64,
    pub created_at: String,
    pub dl_path: String,
    pub downloads: i64,
    pub features: HashMap<String, Vec<String>>,
    pub has_lib: bool,
    pub id: i32,
    pub lib_links: Option<HashMap<String, String>>,
    pub license: Option<String>,
    pub links: HashMap<String, Option<String>>,
    pub num: semver::Version,
    pub published_by: Option<User>,
    pub readme_path: String,
    pub rust_version: Option<String>,
    pub updated_at: String,
    pub yanked: bool,
}

#[derive(Deserialize)]
pub struct CrateInfo {
    pub categories: Vec<String>,
    #[serde(rename = "crate")]
    crate_: Crate,
    pub keywords: Vec<String>,
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

pub struct CratesIo;

impl Default for CratesIo {
    fn default() -> Self {
        Self::new()
    }
}

impl CratesIo {
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
