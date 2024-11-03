use crate::{ProviderError, UpstreamDatum, UpstreamDatumWithMetadata, UpstreamMetadata};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct NpmVersion {
    #[serde(rename = "dist")]
    pub dist: NpmDist,
    #[serde(rename = "dependencies")]
    pub dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "devDependencies")]
    pub dev_dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "peerDependencies")]
    pub peer_dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "optionalDependencies")]
    pub optional_dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "bundledDependencies")]
    pub bundled_dependencies: Option<Vec<String>>,
    #[serde(rename = "engines")]
    pub engines: Option<HashMap<String, String>>,
    #[serde(rename = "scripts")]
    pub scripts: Option<HashMap<String, String>>,
    pub name: String,
    pub version: String,
    #[serde(rename = "readmeFilename")]
    pub readme_filename: Option<String>,
    #[serde(rename = "maintainers")]
    pub maintainers: Vec<NpmPerson>,
    #[serde(rename = "author")]
    pub author: Option<NpmPerson>,
    #[serde(rename = "repository")]
    pub repository: Option<NpmRepository>,
    #[serde(rename = "bugs")]
    pub bugs: Option<NpmBugs>,
    #[serde(rename = "homepage")]
    pub homepage: Option<String>,
    #[serde(rename = "keywords")]
    pub keywords: Option<Vec<String>>,
    #[serde(rename = "license")]
    pub license: Option<String>,
}

#[derive(Deserialize)]
pub struct NpmPerson {
    pub name: String,
    pub email: String,
}

impl From<NpmPerson> for crate::Person {
    fn from(person: NpmPerson) -> Self {
        crate::Person {
            name: Some(person.name),
            email: Some(person.email),
            url: None,
        }
    }
}

#[derive(Deserialize)]
pub struct NpmDist {
    pub shasum: String,
    pub tarball: String,
    pub integrity: String,
    pub signatures: Vec<NpmSignature>,
}

#[derive(Deserialize)]
pub struct NpmSignature {
    pub keyid: String,
    pub sig: String,
}

#[derive(Deserialize)]
pub struct NpmRepository {
    #[serde(rename = "type")]
    pub type_: String,
    pub url: String,
}

#[derive(Deserialize)]
pub struct NpmBugs {
    pub url: String,
}

#[derive(Deserialize)]
pub struct NpmPackage {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_rev")]
    pub rev: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "dist-tags")]
    pub dist_tags: HashMap<String, String>,
    pub versions: HashMap<String, NpmVersion>,
    pub readme: String,
    pub maintainers: Vec<NpmPerson>,
    pub time: HashMap<String, String>,
    pub author: Option<NpmPerson>,
    pub repository: Option<NpmRepository>,
    pub bugs: Option<NpmBugs>,
    pub homepage: Option<String>,
    pub keywords: Option<Vec<String>>,
    pub license: Option<String>,
    pub dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "devDependencies")]
    pub dev_dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "peerDependencies")]
    pub peer_dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "optionalDependencies")]
    pub optional_dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "bundledDependencies")]
    pub bundled_dependencies: Option<Vec<String>>,
    pub engines: Option<HashMap<String, String>>,
    pub scripts: Option<HashMap<String, String>>,
    #[serde(rename = "readmeFilename")]
    pub readme_filename: Option<String>,
}

impl TryInto<UpstreamMetadata> for NpmPackage {
    type Error = ProviderError;

    fn try_into(self) -> Result<UpstreamMetadata, Self::Error> {
        let mut metadata = UpstreamMetadata::default();

        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(self.name.clone()),
            certainty: None,
            origin: None,
        });

        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Description(self.description),
            certainty: None,
            origin: None,
        });

        if let Some(homepage) = self.homepage {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Homepage(homepage),
                certainty: None,
                origin: None,
            });
        }

        if let Some(author) = self.author {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Author(vec![author.into()]),
                certainty: None,
                origin: None,
            });
        }

        if let Some(repository) = self.repository {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(repository.url),
                certainty: None,
                origin: None,
            });
        }

        if let Some(bugs) = self.bugs {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::BugDatabase(bugs.url),
                certainty: None,
                origin: None,
            });
        }

        if let Some(license) = self.license {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::License(license),
                certainty: None,
                origin: None,
            });
        }

        if let Some(keywords) = self.keywords {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Keywords(keywords),
                certainty: None,
                origin: None,
            });
        }

        // Find the latest version
        if let Some(latest_version) = self.dist_tags.get("latest") {
            if let Some(version) = self.versions.get(latest_version) {
                metadata.insert(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(version.version.clone()),
                    certainty: None,
                    origin: None,
                });
            }

            let version_data = self.versions.get(latest_version).map_or_else(
                || {
                    Err(ProviderError::Other(format!(
                        "Could not find version {} in package {}",
                        latest_version, &self.name
                    )))
                },
                Ok,
            )?;

            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Download(version_data.dist.tarball.clone()),
                certainty: None,
                origin: None,
            });
        }

        Ok(metadata)
    }
}

pub async fn load_npm_package(package: &str) -> Result<Option<NpmPackage>, crate::ProviderError> {
    let http_url = format!("https://registry.npmjs.org/{}", package)
        .parse()
        .unwrap();
    let data = crate::load_json_url(&http_url, None).await?;
    Ok(serde_json::from_value(data).unwrap())
}

pub async fn remote_npm_metadata(package: &str) -> Result<UpstreamMetadata, ProviderError> {
    let data = load_npm_package(package).await?;

    match data {
        Some(data) => data.try_into(),
        None => Ok(UpstreamMetadata::default()),
    }
}

#[cfg(test)]
mod npm_tests {
    use super::*;

    #[test]
    fn test_load_npm_package() {
        let data = include_str!(".././testdata/npm.json");

        let npm_data: NpmPackage = serde_json::from_str(data).unwrap();

        assert_eq!(npm_data.name, "leftpad");
    }
}
