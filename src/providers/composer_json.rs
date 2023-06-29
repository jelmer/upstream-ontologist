use crate::{Certainty, UpstreamDatum, UpstreamDatumWithMetadata};
use log::error;
use std::path::Path;

pub fn guess_from_composer_json(
    path: &Path,
    _trust_package: bool,
) -> Vec<UpstreamDatumWithMetadata> {
    // https://getcomposer.org/doc/04-schema.md
    let file = std::fs::File::open(path).expect("Failed to open composer.json");
    let package: serde_json::Value =
        serde_json::from_reader(file).expect("Failed to parse composer.json");

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();

    for (field, value) in package.as_object().unwrap() {
        match field.as_str() {
            "name" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some("composer.json".to_string()),
                });
            }
            "homepage" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Homepage(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some("composer.json".to_string()),
                });
            }
            "description" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some("composer.json".to_string()),
                });
            }
            "license" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some("composer.json".to_string()),
                });
            }
            "version" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some("composer.json".to_string()),
                });
            }
            "type" => {
                if value != "project" {
                    error!("unexpected composer.json type: {:?}", value);
                }
            }
            "keywords" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Keywords(
                        value
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|v| v.as_str().unwrap().to_string())
                            .collect(),
                    ),
                    certainty: Some(Certainty::Certain),
                    origin: Some("composer.json".to_string()),
                });
            }
            "require" | "require-dev" | "autoload" | "autoload-dev" | "scripts" | "extra"
            | "config" | "prefer-stable" | "minimum-stability" => {
                // Do nothing, skip these fields
            }
            _ => {
                error!("Unknown field {} ({:?}) in composer.json", field, value);
            }
        }
    }

    upstream_data
}
