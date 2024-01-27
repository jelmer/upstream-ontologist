use crate::{Certainty, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use log::debug;

pub fn guess_from_cargo(
    path: &std::path::Path,
    trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    // see https://doc.rust-lang.org/cargo/reference/manifest.html
    let doc: toml::Table = toml::from_str(&std::fs::read_to_string(path)?)
        .map_err(|e| ProviderError::ParseError(e.to_string()))?;

    let package = doc
        .get("package")
        .ok_or_else(|| ProviderError::ParseError("No [package] section in Cargo.toml".to_string()))?
        .as_table()
        .ok_or_else(|| {
            ProviderError::ParseError("[package] section in Cargo.toml is not a table".to_string())
        })?;

    let mut results = Vec::new();

    for (field, value) in package.into_iter() {
        match field.as_str() {
            "name" => {
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
                });
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::CargoCrate(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            "description" => {
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            "homepage" => {
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Homepage(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            "license" => {
                let license = value.as_str().unwrap();
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License(license.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            "repository" => {
                let repository = value.as_str().unwrap();
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(repository.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            "version" => {
                let version = value.as_str().unwrap();
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(version.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            "authors" => {
                let authors = value.as_array().unwrap();
                let authors = authors
                    .into_iter()
                    .map(|a| Person::from(a.as_str().unwrap()))
                    .collect();
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Author(authors),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
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
