use crate::{
    Certainty, GuesserSettings, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata,
};
use log::error;
use std::path::Path;
use url::Url;

/// Extracts upstream metadata from NPM package.json file
pub fn guess_from_package_json(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    // see https://docs.npmjs.com/cli/v7/configuring-npm/package-json
    let file = std::fs::File::open(path)?;
    let package: serde_json::Value =
        serde_json::from_reader(file).map_err(|e| ProviderError::ParseError(e.to_string()))?;

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();

    let package = match package {
        serde_json::Value::Object(package) => package,
        _ => {
            return Err(ProviderError::ParseError(
                "package.json is not an object".to_string(),
            ));
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
                    log::warn!("package.json: expected string for 'name', got {:?}", value);
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
                        "package.json: expected string for 'homepage', got {:?}",
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
                        "package.json: expected string for 'description', got {:?}",
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
                        "package.json: expected string for 'license', got {:?}",
                        value
                    );
                }
            }
            "demo" => {
                if let Some(s) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Demo(s.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                } else {
                    log::warn!("package.json: expected string for 'demo', got {:?}", value);
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
                        "package.json: expected string for 'version', got {:?}",
                        value
                    );
                }
            }
            "repository" => {
                let repo_url = if let Some(repo_url) = value.as_str() {
                    Some(repo_url)
                } else if let Some(repo) = value.as_object() {
                    if let Some(repo_url) = repo.get("url") {
                        repo_url.as_str()
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(repo_url) = repo_url {
                    match Url::parse(repo_url) {
                        Ok(url) if url.scheme() == "github" => {
                            // Some people seem to default to github. :(
                            let repo_url = format!("https://github.com/{}", url.path());
                            upstream_data.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::Repository(repo_url.to_string()),
                                certainty: Some(Certainty::Likely),
                                origin: Some(path.into()),
                            });
                        }
                        Err(url::ParseError::RelativeUrlWithoutBase) => {
                            // Some people seem to default to github. :(
                            let repo_url = format!("https://github.com/{}", repo_url);
                            upstream_data.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::Repository(repo_url.to_string()),
                                certainty: Some(Certainty::Likely),
                                origin: Some(path.into()),
                            });
                        }
                        Ok(url) => {
                            upstream_data.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::Repository(url.to_string()),
                                certainty: Some(Certainty::Certain),
                                origin: Some(path.into()),
                            });
                        }
                        Err(e) => {
                            log::warn!(
                                "package.json: failed to parse repository URL {:?}: {}",
                                repo_url,
                                e
                            );
                        }
                    }
                }
            }
            "bugs" => {
                if let Some(url) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::BugDatabase(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                } else if let Some(email) = value.get("email").and_then(serde_json::Value::as_str) {
                    let url = format!("mailto:{}", email);
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::BugDatabase(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
            "keywords" => {
                if let Some(keywords) = value.as_array() {
                    let keywords = keywords
                        .iter()
                        .filter_map(|keyword| keyword.as_str())
                        .map(String::from)
                        .collect();
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Keywords(keywords),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
            "author" => {
                if let Some(author) = value.as_object() {
                    let name = author
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .map(String::from);
                    let url = author
                        .get("url")
                        .and_then(serde_json::Value::as_str)
                        .map(String::from);
                    let email = author
                        .get("email")
                        .and_then(serde_json::Value::as_str)
                        .map(String::from);
                    let person = Person { name, url, email };
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Author(vec![person]),
                        certainty: Some(Certainty::Confident),
                        origin: Some(path.into()),
                    });
                } else if let Some(author) = value.as_str() {
                    let person = Person::from(author);
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Author(vec![person]),
                        certainty: Some(Certainty::Confident),
                        origin: Some(path.into()),
                    });
                } else {
                    error!("Unsupported type for author in package.json: {:?}", value);
                }
            }
            "dependencies" | "private" | "devDependencies" | "scripts" | "files" | "main" => {
                // Do nothing, skip these fields
            }
            _ => {
                error!("Unknown package.json field {} ({:?})", field, value);
            }
        }
    }

    Ok(upstream_data)
}

#[cfg(test)]
mod package_json_tests {
    use super::*;

    fn write_json(td: &tempfile::TempDir, content: &str) -> std::path::PathBuf {
        let path = td.path().join("package.json");
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_wrong_field_types() {
        let td = tempfile::tempdir().unwrap();
        let path = write_json(
            &td,
            r#"{"name": 42, "homepage": [], "license": {}, "version": null}"#,
        );
        let result = guess_from_package_json(&path, &GuesserSettings::default()).unwrap();
        assert_eq!(result, vec![]);
    }

    #[test]
    fn test_relative_repository_url_guesses_github() {
        let td = tempfile::tempdir().unwrap();
        let path = write_json(&td, r#"{"repository": "user/repo"}"#);
        let result = guess_from_package_json(&path, &GuesserSettings::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0].datum,
            UpstreamDatum::Repository(url) if url == "https://github.com/user/repo"
        ));
    }

    #[test]
    fn test_not_an_object() {
        let td = tempfile::tempdir().unwrap();
        let path = write_json(&td, r#"[1, 2, 3]"#);
        let result = guess_from_package_json(&path, &GuesserSettings::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_dummy() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("package.json");

        std::fs::write(
            &path,
            r#"{
  "name": "mozillaeslintsetup",
  "description": "This package file is for setup of ESLint.",
  "repository": {},
  "license": "MPL-2.0",
  "dependencies": {
    "eslint": "4.18.1",
    "eslint-plugin-html": "4.0.2",
    "eslint-plugin-mozilla": "file:tools/lint/eslint/eslint-plugin-mozilla",
    "eslint-plugin-no-unsanitized": "2.0.2",
    "eslint-plugin-react": "7.1.0",
    "eslint-plugin-spidermonkey-js":
        "file:tools/lint/eslint/eslint-plugin-spidermonkey-js"
  },
  "devDependencies": {}
}
"#,
        )
        .unwrap();
        let ret = guess_from_package_json(&path, &GuesserSettings::default()).unwrap();
        assert_eq!(
            ret,
            vec![
                UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(
                        "This package file is for setup of ESLint.".to_string()
                    ),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.clone().into()),
                },
                UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License("MPL-2.0".to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.clone().into())
                },
                UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name("mozillaeslintsetup".to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into())
                }
            ]
        );
    }
}
