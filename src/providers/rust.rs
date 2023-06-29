use crate::{Certainty, Person, UpstreamDatum, UpstreamDatumWithMetadata};
use log::debug;

pub fn guess_from_cargo(
    path: &std::path::Path,
    trust_package: bool,
) -> Vec<UpstreamDatumWithMetadata> {
    // see https://doc.rust-lang.org/cargo/reference/manifest.html
    let doc = toml::from_str(&std::fs::read_to_string(path).expect("Failed to read Cargo.toml"));

    let doc: toml::Table = if let Ok(doc) = doc {
        doc
    } else {
        return Vec::new();
    };

    let package = doc.get("package").unwrap().as_table().unwrap();

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

    results
}
