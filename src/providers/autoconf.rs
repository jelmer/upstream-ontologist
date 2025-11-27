use crate::{Certainty, GuesserSettings, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use log::debug;
use std::fs::File;
use std::io::{BufRead, BufReader};
use url::Url;

fn is_email_address(email: &str) -> bool {
    if email.contains('@') {
        return true;
    }

    if email.contains(" (at) ") {
        return true;
    }

    false
}

/// Extracts upstream metadata from autoconf configure script
pub fn guess_from_configure(
    path: &std::path::Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    if std::path::Path::new(path).is_dir() {
        return Ok(Vec::new());
    }

    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut results = Vec::new();

    for line in reader.split(b'\n').map_while(Result::ok) {
        let split = line.splitn(2, |&c| c == b'=').collect::<Vec<_>>();
        let (key, value) = if let [key, value] = split.as_slice() {
            (key, value)
        } else {
            continue;
        };
        let key = String::from_utf8(key.to_vec()).expect("Failed to parse UTF-8");
        let key = key.trim();
        let value = String::from_utf8(value.to_vec()).expect("Failed to parse UTF-8");
        let mut value = value.trim();

        if key.contains(' ') {
            continue;
        }

        if value.contains('$') {
            continue;
        }

        if value.starts_with('\'') && value.ends_with('\'') {
            if value.len() >= 2 {
                value = &value[1..value.len() - 1];
                if value.is_empty() {
                    continue;
                }
            } else {
                // Single quote character, skip it
                continue;
            }
        }

        match key {
            "PACKAGE_NAME" => {
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(value.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
            "PACKAGE_TARNAME" => {
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(value.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
            "PACKAGE_VERSION" => {
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(value.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
            "PACKAGE_BUGREPORT" => {
                let certainty = if value == "BUG-REPORT-ADDRESS" {
                    None
                } else if is_email_address(value) {
                    // Downgrade the trustworthiness of this field for most
                    // upstreams if it contains an e-mail address. Most
                    // upstreams seem to just set this to some random address,
                    // and then forget about it.
                    Some(Certainty::Possible)
                } else if value.contains("mailing list") {
                    // Downgrade the trustworthiness of this field if
                    // it contains a mailing list
                    Some(Certainty::Possible)
                } else {
                    let parsed_url = Url::parse(value).expect("Failed to parse URL");
                    if !parsed_url.path().trim_end_matches('/').is_empty() {
                        Some(Certainty::Certain)
                    } else {
                        // It seems unlikely that the bug submit URL lives at
                        // the root.
                        Some(Certainty::Possible)
                    }
                };

                if certainty.is_some() {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::BugSubmit(value.to_string()),
                        certainty,
                        origin: Some(path.into()),
                    });
                }
            }
            "PACKAGE_URL" => {
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Homepage(value.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
            _ => {
                debug!("unknown key: {}", key);
            }
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_single_quote_value() {
        // Test that a single quote character doesn't cause a panic
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "PACKAGE_NAME='").unwrap();

        let settings = GuesserSettings::default();
        let result = guess_from_configure(file.path(), &settings);

        assert!(result.is_ok());
        let data = result.unwrap();
        // Single quote should be skipped, so no results
        assert_eq!(data.len(), 0);
    }

    #[test]
    fn test_empty_quoted_value() {
        // Test that empty quoted strings are skipped
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "PACKAGE_NAME=''").unwrap();

        let settings = GuesserSettings::default();
        let result = guess_from_configure(file.path(), &settings);

        assert!(result.is_ok());
        let data = result.unwrap();
        // Empty quoted value should be skipped
        assert_eq!(data.len(), 0);
    }

    #[test]
    fn test_valid_quoted_value() {
        // Test that properly quoted values are extracted
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "PACKAGE_NAME='my-package'").unwrap();

        let settings = GuesserSettings::default();
        let result = guess_from_configure(file.path(), &settings);

        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data.len(), 1);
        assert!(matches!(data[0].datum, UpstreamDatum::Name(ref name) if name == "my-package"));
    }
}
