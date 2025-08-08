use crate::{Certainty, GuesserSettings, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use log::error;
use std::path::Path;

/// Extracts upstream metadata from PHP composer.json file
pub fn guess_from_composer_json(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    // https://getcomposer.org/doc/04-schema.md
    let file = std::fs::File::open(path)?;
    let package: serde_json::Value =
        serde_json::from_reader(file).map_err(|e| ProviderError::ParseError(e.to_string()))?;

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();

    let package = match package.as_object() {
        Some(package) => package,
        None => {
            return Err(ProviderError::Other(
                "Failed to parse composer.json".to_string(),
            ))
        }
    };

    for (field, value) in package {
        match field.as_str() {
            "name" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
            "homepage" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Homepage(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
            "description" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
            "license" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
            "version" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
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
                    origin: Some(path.into()),
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

    Ok(upstream_data)
}
