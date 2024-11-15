use crate::{
    Certainty, GuesserSettings, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata,
    UpstreamMetadata,
};
use log::debug;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub async fn guess_from_gemspec(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let file = File::open(path)?;

    let reader = BufReader::new(file);
    let mut results = Vec::new();

    #[derive(Debug)]
    enum GemValue {
        String(String),
        Array(Vec<GemValue>),
    }

    impl GemValue {
        fn as_str(&self) -> Option<&str> {
            match self {
                GemValue::String(s) => Some(s),
                GemValue::Array(_) => None,
            }
        }

        fn as_array(&self) -> Option<&Vec<GemValue>> {
            match self {
                GemValue::String(_) => None,
                GemValue::Array(a) => Some(a),
            }
        }
    }

    fn parse_value(value: &str) -> Result<GemValue, String> {
        let trimmed = value.trim();
        if (trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        {
            return Ok(GemValue::String(trimmed[1..trimmed.len() - 1].to_string()));
        } else if trimmed.starts_with('"') || trimmed.starts_with("'.freeze") {
            return Ok(GemValue::String(trimmed[1..].to_string()));
        } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let elements = trimmed[1..trimmed.len() - 1]
                .split(',')
                .map(parse_value)
                .collect::<Result<Vec<GemValue>, _>>()?;
            return Ok(GemValue::Array(elements));
        }
        Err(format!("Could not parse value: {}", value))
    }

    for line in reader.lines().map_while(Result::ok) {
        if line.starts_with('#') {
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        if line == "Gem::Specification.new do |s|\n" || line == "end\n" {
            continue;
        }
        if let Some(line) = line.strip_prefix("  s.") {
            let (key, rawval) = match line.split_once('=') {
                Some((key, rawval)) => (key.trim(), rawval),
                _ => continue,
            };

            let val = match parse_value(rawval.trim()) {
                Ok(val) => val,
                Err(_) => {
                    debug!("Could not parse value: {}", rawval);
                    continue;
                }
            };

            match key {
                "name" => results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(val.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                }),
                "version" => results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(val.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                }),
                "homepage" => results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Homepage(val.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                }),
                "summary" => results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(val.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                }),
                "description" => results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Description(val.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                }),
                "license" => results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License(val.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                }),
                "authors" => results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Author(
                        val.as_array()
                            .unwrap()
                            .iter()
                            .map(|p| Person::from(p.as_str().unwrap()))
                            .collect(),
                    ),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                }),
                _ => debug!("unknown field {} ({:?}) in gemspec", key, val),
            }
        } else {
            debug!("ignoring unparsable line in {}: {:?}", path.display(), line);
        }
    }

    Ok(results)
}

#[derive(Deserialize)]
pub struct RubygemMetadata {
    pub changelog_uri: Option<url::Url>,
    pub source_code_uri: Option<url::Url>,
}

#[derive(Deserialize)]
pub struct RubygemDependency {
    pub name: String,
    pub requirements: String,
}

#[derive(Deserialize)]
pub struct RubygemDependencies {
    pub development: Vec<RubygemDependency>,
    pub runtime: Vec<RubygemDependency>,
}

#[derive(Deserialize)]
pub struct Rubygem {
    pub name: String,
    pub downloads: usize,
    pub version: String,
    pub version_created_at: String,
    pub version_downloads: usize,
    pub platform: String,
    pub authors: String,
    pub info: String,
    pub licenses: Vec<String>,
    pub metadata: RubygemMetadata,
    pub yanked: bool,
    pub sha: String,
    pub spec_sha: String,
    pub project_uri: url::Url,
    pub gem_uri: url::Url,
    pub homepage_uri: Option<url::Url>,
    pub wiki_uri: Option<url::Url>,
    pub documentation_uri: Option<url::Url>,
    pub mailing_list_uri: Option<url::Url>,
    pub source_code_uri: Option<url::Url>,
    pub bug_tracker_uri: Option<url::Url>,
    pub changelog_uri: Option<url::Url>,
    pub funding_uri: Option<url::Url>,
    pub dependencies: RubygemDependencies,
}

impl TryFrom<Rubygem> for UpstreamMetadata {
    type Error = ProviderError;

    fn try_from(gem: Rubygem) -> Result<Self, ProviderError> {
        let mut metadata = UpstreamMetadata::default();
        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(gem.name),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Version(gem.version),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Author(vec![Person::from(gem.authors.as_str())]),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Homepage(gem.homepage_uri.unwrap_or(gem.project_uri).to_string()),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        if let Some(wiki_uri) = gem.wiki_uri {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Wiki(wiki_uri.to_string()),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        if let Some(mailing_list_uri) = gem.mailing_list_uri {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::MailingList(mailing_list_uri.to_string()),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        if let Some(bug_tracker_uri) = gem.bug_tracker_uri {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::BugDatabase(bug_tracker_uri.to_string()),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        if let Some(funding_uri) = gem.funding_uri {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Funding(funding_uri.to_string()),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        if let Some(source_code_uri) = gem.source_code_uri {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(source_code_uri.to_string()),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::License(gem.licenses.join(", ")),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        if let Some(documentation_uri) = gem.documentation_uri {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Documentation(documentation_uri.to_string()),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        if let Some(changelog_uri) = gem.changelog_uri {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Changelog(changelog_uri.to_string()),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        Ok(metadata)
    }
}

pub async fn load_rubygem(name: &str) -> Result<Option<Rubygem>, ProviderError> {
    let url = format!("https://rubygems.org/api/v1/gems/{}.json", name)
        .parse()
        .unwrap();
    let data = crate::load_json_url(&url, None).await?;
    let gem: Rubygem = serde_json::from_value(data).unwrap();
    Ok(Some(gem))
}

pub async fn remote_rubygem_metadata(name: &str) -> Result<UpstreamMetadata, ProviderError> {
    let gem = load_rubygem(name).await?;

    match gem {
        Some(gem) => gem.try_into(),
        None => Ok(UpstreamMetadata::default()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_gem() {
        let gemspec = include_str!("../testdata/rubygem.json");

        let gem: super::Rubygem = serde_json::from_str(gemspec).unwrap();

        assert_eq!(gem.name, "bullet");
    }
}
