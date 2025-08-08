use crate::{ProviderError, UpstreamDatum, UpstreamDatumWithMetadata, UpstreamMetadata};
use serde::Deserialize;
use std::collections::HashMap;

/// Information about a specific NPM package version
#[derive(Deserialize)]
pub struct NpmVersion {
    /// Distribution information for the package
    #[serde(rename = "dist")]
    pub dist: NpmDist,
    /// Package dependencies
    #[serde(rename = "dependencies")]
    pub dependencies: Option<HashMap<String, String>>,
    /// Development dependencies
    #[serde(rename = "devDependencies")]
    pub dev_dependencies: Option<HashMap<String, String>>,
    /// Peer dependencies
    #[serde(rename = "peerDependencies")]
    pub peer_dependencies: Option<HashMap<String, String>>,
    /// Optional dependencies
    #[serde(rename = "optionalDependencies")]
    pub optional_dependencies: Option<HashMap<String, String>>,
    /// Bundled dependencies
    #[serde(rename = "bundledDependencies")]
    pub bundled_dependencies: Option<Vec<String>>,
    /// Engine requirements
    #[serde(rename = "engines")]
    pub engines: Option<HashMap<String, String>>,
    /// NPM scripts
    #[serde(rename = "scripts")]
    pub scripts: Option<HashMap<String, String>>,
    /// Package name
    pub name: String,
    /// Package version
    pub version: String,
    /// README file name
    #[serde(rename = "readmeFilename")]
    pub readme_filename: Option<String>,
    /// Package maintainers
    #[serde(rename = "maintainers")]
    pub maintainers: Vec<NpmPerson>,
    /// Package author
    #[serde(rename = "author")]
    pub author: Option<NpmPerson>,
    /// Source repository
    #[serde(rename = "repository")]
    pub repository: Option<NpmRepository>,
    /// Bug tracker information
    #[serde(rename = "bugs")]
    pub bugs: Option<NpmBugs>,
    /// Package homepage
    #[serde(rename = "homepage")]
    pub homepage: Option<String>,
    /// Package keywords
    #[serde(rename = "keywords")]
    pub keywords: Option<Vec<String>>,
    /// License identifier
    #[serde(rename = "license")]
    pub license: Option<String>,
}

/// NPM person (author/maintainer) information
#[derive(Deserialize)]
pub struct NpmPerson {
    /// Person's name
    pub name: String,
    /// Person's email address
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

/// NPM distribution information
#[derive(Deserialize)]
pub struct NpmDist {
    /// SHA checksum of the package
    pub shasum: String,
    /// URL to the package tarball
    pub tarball: String,
    /// Package integrity string
    pub integrity: String,
    /// Package signatures
    pub signatures: Vec<NpmSignature>,
}

/// NPM package signature
#[derive(Deserialize)]
pub struct NpmSignature {
    /// Key identifier
    pub keyid: String,
    /// Signature string
    pub sig: String,
}

/// NPM repository information
#[derive(Deserialize)]
pub struct NpmRepository {
    /// Repository type (e.g., git)
    #[serde(rename = "type")]
    pub type_: String,
    /// Repository URL
    pub url: String,
}

/// NPM bug tracker information
#[derive(Deserialize)]
pub struct NpmBugs {
    /// Bug tracker URL
    pub url: String,
}

/// Complete NPM package metadata
#[derive(Deserialize)]
pub struct NpmPackage {
    /// Package identifier
    #[serde(rename = "_id")]
    pub id: String,
    /// Package revision
    #[serde(rename = "_rev")]
    pub rev: String,
    /// Package name
    pub name: String,
    /// Package description
    pub description: String,
    /// Distribution tags mapping
    #[serde(rename = "dist-tags")]
    pub dist_tags: HashMap<String, String>,
    /// All available versions
    pub versions: HashMap<String, NpmVersion>,
    /// Package README content
    pub readme: String,
    /// Package maintainers
    pub maintainers: Vec<NpmPerson>,
    /// Timestamps for various events
    pub time: HashMap<String, String>,
    /// Package author
    pub author: Option<NpmPerson>,
    /// Source repository
    pub repository: Option<NpmRepository>,
    /// Bug tracker information
    pub bugs: Option<NpmBugs>,
    /// Package homepage
    pub homepage: Option<String>,
    /// Package keywords
    pub keywords: Option<Vec<String>>,
    /// License identifier
    pub license: Option<String>,
    /// Package dependencies
    pub dependencies: Option<HashMap<String, String>>,
    /// Development dependencies
    #[serde(rename = "devDependencies")]
    pub dev_dependencies: Option<HashMap<String, String>>,
    /// Peer dependencies
    #[serde(rename = "peerDependencies")]
    pub peer_dependencies: Option<HashMap<String, String>>,
    /// Optional dependencies
    #[serde(rename = "optionalDependencies")]
    pub optional_dependencies: Option<HashMap<String, String>>,
    /// Bundled dependencies
    #[serde(rename = "bundledDependencies")]
    pub bundled_dependencies: Option<Vec<String>>,
    /// Engine requirements
    pub engines: Option<HashMap<String, String>>,
    /// NPM scripts
    pub scripts: Option<HashMap<String, String>>,
    /// README file name
    #[serde(rename = "readmeFilename")]
    pub readme_filename: Option<String>,
}

impl TryInto<UpstreamMetadata> for NpmPackage {
    type Error = ProviderError;

    fn try_into(self) -> Result<UpstreamMetadata, Self::Error> {
        let mut metadata = UpstreamMetadata::default();

        let package_name = self.name.clone();
        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(self.name),
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
                    datum: UpstreamDatum::Version(version.version.to_string()),
                    certainty: None,
                    origin: None,
                });
            }

            let version_data = self.versions.get(latest_version).map_or_else(
                || {
                    Err(ProviderError::Other(format!(
                        "Could not find version {} in package {}",
                        latest_version, &package_name
                    )))
                },
                Ok,
            )?;

            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Download(version_data.dist.tarball.to_string()),
                certainty: None,
                origin: None,
            });
        }

        Ok(metadata)
    }
}

/// Load NPM package metadata from the registry
///
/// Fetches package information from the NPM registry API for the specified package name.
/// Returns the parsed package metadata or None if the package doesn't exist.
pub async fn load_npm_package(package: &str) -> Result<Option<NpmPackage>, crate::ProviderError> {
    let http_url = format!("https://registry.npmjs.org/{}", package)
        .parse()
        .unwrap();
    let data = crate::load_json_url(&http_url, None).await?;
    Ok(serde_json::from_value(data).unwrap())
}

/// Get upstream metadata for an NPM package
///
/// Retrieves and converts NPM package information into standardized upstream metadata format.
/// Returns empty metadata if the package is not found.
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
