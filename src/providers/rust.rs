use crate::{
    Certainty, GuesserSettings, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata,
};
use log::debug;

pub fn guess_from_cargo(
    path: &std::path::Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    // see https://doc.rust-lang.org/cargo/reference/manifest.html
    let doc: toml::Table = toml::from_str(&std::fs::read_to_string(path)?)
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

fn parse_crates_io(data: serde_json::Value) -> Vec<UpstreamDatum> {
    let crate_data = &data["crate"];
    let mut results = Vec::new();
    results.push(UpstreamDatum::Name(
        crate_data["name"].as_str().unwrap().to_string(),
    ));
    if let Some(homepage) = crate_data.get("homepage") {
        results.push(UpstreamDatum::Homepage(
            homepage.as_str().unwrap().to_string(),
        ));
    }
    if let Some(repository) = crate_data.get("repository") {
        results.push(UpstreamDatum::Repository(
            repository.as_str().unwrap().to_string(),
        ));
    }
    if let Some(description) = crate_data.get("description") {
        results.push(UpstreamDatum::Summary(
            description.as_str().unwrap().to_string(),
        ));
    }
    if let Some(license) = crate_data.get("license") {
        results.push(UpstreamDatum::License(
            license.as_str().unwrap().to_string(),
        ));
    }
    if let Some(version) = crate_data.get("newest_version") {
        results.push(UpstreamDatum::Version(
            version.as_str().unwrap().to_string(),
        ));
    }

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
        let data = crate::load_json_url(
            &format!("https://crates.io/api/v1/crates/{}", name)
                .parse()
                .unwrap(),
            None,
        )?;
        Ok(parse_crates_io(data))
    }
}
