use std::collections::HashMap;
use serde::Deserialize;

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
    pub keywords: Vec<String>,
    #[serde(rename = "license")]
    pub license: Option<String>,
}

#[derive(Deserialize)]
pub struct NpmPerson {
    pub name: String,
    pub email: String,
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
    pub keywords: Vec<String>,
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

pub fn load_npm_package(package: &str) -> Result<Option<NpmPackage>, crate::ProviderError> {
    let http_url = format!("https://registry.npmjs.org/{}", package).parse().unwrap();
    let data = crate::load_json_url(&http_url, None)?;
    Ok(serde_json::from_value(data).unwrap())
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
