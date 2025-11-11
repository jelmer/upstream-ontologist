use crate::{
    Certainty, GuesserSettings, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata,
};
use log::warn;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Extracts upstream metadata from metadata.json file
pub fn guess_from_metadata_json(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let data: serde_json::Map<String, serde_json::Value> = match serde_json::from_str(&contents) {
        Ok(data) => data,
        Err(e) => {
            return Err(ProviderError::ParseError(e.to_string()));
        }
    };

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();

    for (field, value) in data.iter() {
        match field.as_str() {
            "description" => {
                if let Some(description) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Description(description.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
            "name" => {
                if let Some(name) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Name(name.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
            "version" => {
                if let Some(version) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Version(version.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
            "url" => {
                if let Some(url) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Homepage(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
            "license" => {
                if let Some(license) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::License(license.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
            "source" => {
                if let Some(repository) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Repository(repository.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                } else if let Some(array) = value.as_array() {
                    // Handle Repository as array (e.g., CVS format: ["cvs_root", "module"])
                    let strings: Vec<&str> = array.iter().filter_map(|v| v.as_str()).collect();

                    if strings.len() >= 2 {
                        // Try to convert CVS array format
                        if let Some(repo_url) = crate::vcs::convert_cvs_list_to_str(&strings) {
                            upstream_data.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::Repository(repo_url),
                                certainty: Some(Certainty::Certain),
                                origin: Some(path.into()),
                            });
                        } else {
                            warn!("Repository array format not recognized: {:?}", strings);
                        }
                    } else {
                        warn!("Repository array has insufficient elements: {:?}", array);
                    }
                }
            }
            "summary" => {
                if let Some(summary) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Summary(summary.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
            "issues_url" => {
                if let Some(issues_url) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::BugDatabase(issues_url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
            "project_page" => {
                if let Some(project_page) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Homepage(project_page.to_string()),
                        certainty: Some(Certainty::Likely),
                        origin: Some(path.into()),
                    });
                }
            }
            "author" => {
                if let Some(author_value) = value.as_str() {
                    let author = Person::from(author_value);
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Author(vec![author]),
                        certainty: Some(Certainty::Likely),
                        origin: Some(path.into()),
                    });
                } else if let Some(author_values) = value.as_array() {
                    let authors: Vec<Person> = match author_values
                        .iter()
                        .map(|v| {
                            Ok::<Person, &str>(Person::from(
                                v.as_str().ok_or("Author value is not a string")?,
                            ))
                        })
                        .collect::<std::result::Result<Vec<_>, _>>()
                    {
                        Ok(authors) => authors,
                        Err(e) => {
                            warn!("Error parsing author array: {}", e);
                            continue;
                        }
                    };
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Author(authors),
                        certainty: Some(Certainty::Likely),
                        origin: Some(path.into()),
                    });
                }
            }
            "operatingsystem_support" | "requirements" | "dependencies" => {
                // Skip these fields
            }
            _ => {
                warn!("Unknown field {} ({:?}) in metadata.json", field, value);
            }
        }
    }

    Ok(upstream_data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_cvs_repository_array() {
        let json_content = r#"{
  "name": "yep",
  "source": [
    ":extssh:_anoncvs@anoncvs.example.org:/cvs",
    "yep"
  ]
}"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(json_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let settings = GuesserSettings {
            trust_package: false,
        };
        let result = guess_from_metadata_json(temp_file.path(), &settings).unwrap();

        // Find the Repository datum
        let repo = result
            .iter()
            .find(|d| matches!(d.datum, UpstreamDatum::Repository(_)))
            .expect("Should have Repository datum");

        if let UpstreamDatum::Repository(url) = &repo.datum {
            println!("Converted CVS URL: {}", url);
            // The URL should be converted from CVS array format
            assert!(
                url.contains("anoncvs.example.org"),
                "URL should contain the host"
            );
            assert!(url.contains("yep"), "URL should contain the module name");
        } else {
            panic!("Expected Repository datum");
        }

        assert_eq!(repo.certainty, Some(Certainty::Certain));
    }
}
