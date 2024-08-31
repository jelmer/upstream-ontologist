use crate::{
    Certainty, GuesserSettings, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata,
    UpstreamMetadata,
};
use log::debug;
use serde::Deserialize;
use std::collections::HashMap;
use toml::value::Table;

pub fn guess_from_cargo(
    path: &std::path::Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    // see https://doc.rust-lang.org/cargo/reference/manifest.html
    let doc: Table = toml::from_str(&std::fs::read_to_string(path)?)
        .map_err(|e| ProviderError::ParseError(e.to_string()))?;

    let package = match doc.get("package") {
        Some(package) => package.as_table().ok_or_else(|| {
            ProviderError::ParseError("[package] section in Cargo.toml is not a table".to_string())
        })?,
        None => {
            log::debug!("No [package] section in Cargo.toml");
            return Ok(Vec::new());
        }
    };

    let mut results = Vec::new();

    for (field, value) in package.into_iter() {
        match field.as_str() {
            "name" => {
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::CargoCrate(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
            "description" => {
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
            "homepage" => {
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Homepage(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
            "license" => {
                let license = value.as_str().unwrap();
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License(license.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
            "repository" => {
                let repository = value.as_str().unwrap();
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(repository.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
            "version" => {
                if let Some(version) = value.as_str() {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Version(version.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
            "authors" => {
                let authors = value.as_array().unwrap();
                let authors = authors
                    .iter()
                    .map(|a| Person::from(a.as_str().unwrap()))
                    .collect();
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Author(authors),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
            "edition" | "default-run" => {}
            n => {
                debug!("Unknown Cargo.toml field: {}", n);
            }
        }
    }

    Ok(results)
}

pub fn cargo_translate_dashes(crate_name: &str) -> Result<Option<String>, crate::HTTPJSONError> {
    let url = format!("https://crates.io/api/v1/crates?q={}", crate_name)
        .parse()
        .unwrap();
    let json: serde_json::Value = crate::load_json_url(&url, None)?;

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
    pub description: String,
    pub documentation: String,
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
    pub versions: Vec<i32>,
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

        ret.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Summary(value.crate_.description.to_string()),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

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

pub fn load_crate_info(cratename: &str) -> Result<Option<CrateInfo>, crate::ProviderError> {
    let http_url = format!("https://crates.io/api/v1/crates/{}", cratename);

    let data = crate::load_json_url(&http_url.parse().unwrap(), None)?;

    Ok(Some(serde_json::from_value(data).unwrap()))
}

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
    results.push(UpstreamDatum::Summary(crate_data.description.to_string()));
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

    fn guess_metadata(&self, name: &str) -> Result<Vec<UpstreamDatum>, ProviderError> {
        let data = load_crate_info(name)?;
        if data.is_none() {
            return Ok(Vec::new());
        }
        Ok(parse_crates_io(&data.unwrap()))
    }
}

pub fn remote_crate_data(name: &str) -> Result<UpstreamMetadata, crate::ProviderError> {
    let data = load_crate_info(name)?;

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
