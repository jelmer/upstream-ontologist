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
                if let Some(s) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Name(s.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                } else {
                    log::warn!("composer.json: expected string for 'name', got {:?}", value);
                }
            }
            "homepage" => {
                if let Some(s) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Homepage(s.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                } else {
                    log::warn!(
                        "composer.json: expected string for 'homepage', got {:?}",
                        value
                    );
                }
            }
            "description" => {
                if let Some(s) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Summary(s.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                } else {
                    log::warn!(
                        "composer.json: expected string for 'description', got {:?}",
                        value
                    );
                }
            }
            "license" => {
                if let Some(s) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::License(s.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                } else {
                    log::warn!(
                        "composer.json: expected string for 'license', got {:?}",
                        value
                    );
                }
            }
            "version" => {
                if let Some(s) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Version(s.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                } else {
                    log::warn!(
                        "composer.json: expected string for 'version', got {:?}",
                        value
                    );
                }
            }
            "type" => {
                if value != "project" {
                    error!("unexpected composer.json type: {:?}", value);
                }
            }
            "keywords" => {
                if let Some(arr) = value.as_array() {
                    let keywords: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Keywords(keywords),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                } else {
                    log::warn!(
                        "composer.json: expected array for 'keywords', got {:?}",
                        value
                    );
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_json(td: &tempfile::TempDir, content: &str) -> std::path::PathBuf {
        let path = td.path().join("composer.json");
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_wrong_field_types() {
        let td = tempfile::tempdir().unwrap();
        let path = write_json(
            &td,
            r#"{"name": 42, "homepage": true, "license": ["MIT"], "version": null}"#,
        );
        let result = guess_from_composer_json(&path, &GuesserSettings::default()).unwrap();
        assert_eq!(result, vec![]);
    }

    #[test]
    fn test_keywords_wrong_type() {
        let td = tempfile::tempdir().unwrap();
        let path = write_json(&td, r#"{"keywords": "not-an-array"}"#);
        let result = guess_from_composer_json(&path, &GuesserSettings::default()).unwrap();
        assert_eq!(result, vec![]);
    }

    #[test]
    fn test_keywords_mixed_types() {
        let td = tempfile::tempdir().unwrap();
        let path = write_json(&td, r#"{"keywords": ["valid", 123, "also-valid"]}"#);
        let result = guess_from_composer_json(&path, &GuesserSettings::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert!(
            matches!(&result[0].datum, UpstreamDatum::Keywords(k) if k == &["valid", "also-valid"])
        );
    }

    #[test]
    fn test_not_an_object() {
        let td = tempfile::tempdir().unwrap();
        let path = write_json(&td, r#""just a string""#);
        let result = guess_from_composer_json(&path, &GuesserSettings::default());
        assert!(result.is_err());
    }
}
