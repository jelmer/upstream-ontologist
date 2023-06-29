use crate::{Certainty, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use log::debug;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub fn guess_from_gemspec(
    path: &Path,
    trust_package: bool,
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

    for line in reader.lines().flatten() {
        if line.starts_with('#') {
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        if line == "Gem::Specification.new do |s|\n" || line == "end\n" {
            continue;
        }
        if line.starts_with("  s.") {
            let (key, rawval) = match line[4..].split_once('=') {
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
                    origin: Some(path.to_string_lossy().to_string()),
                }),
                "version" => results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(val.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
                }),
                "homepage" => results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Homepage(val.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
                }),
                "summary" => results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(val.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
                }),
                "description" => results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Description(val.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
                }),
                "license" => results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License(val.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
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
                    origin: Some(path.to_string_lossy().to_string()),
                }),
                _ => debug!("unknown field {} ({:?}) in gemspec", key, val),
            }
        } else {
            debug!(
                "ignoring unparseable line in {}: {:?}",
                path.display(),
                line
            );
        }
    }

    Ok(results)
}
