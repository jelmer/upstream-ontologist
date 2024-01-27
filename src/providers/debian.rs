use crate::{
    bug_database_from_issue_url, repo_url_from_merge_request_url, Certainty, Person, ProviderError,
    UpstreamDatum, UpstreamDatumWithMetadata,
};
use log::debug;
use std::fs::File;
use std::io::BufRead;
use std::path::Path;
use url::Url;

pub fn guess_from_debian_patch(
    path: &Path,
    trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let file = File::open(path)?;
    let reader = std::io::BufReader::new(file);

    let net_access = None;

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();

    for line in reader.lines().flatten() {
        if line.starts_with("Forwarded: ") {
            let forwarded = match line.split_once(':') {
                Some((_, url)) => url.trim(),
                None => {
                    debug!("Malformed Forwarded line in patch {}", path.display());
                    continue;
                }
            };
            let forwarded = match Url::parse(forwarded) {
                Ok(url) => url,
                Err(e) => {
                    debug!(
                        "Malformed URL in Forwarded line in patch {}: {}",
                        path.display(),
                        e
                    );
                    continue;
                }
            };

            if let Some(bug_db) = bug_database_from_issue_url(&forwarded, net_access) {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::BugDatabase(bug_db.to_string()),
                    certainty: Some(Certainty::Possible),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }

            if let Some(repo_url) = repo_url_from_merge_request_url(&forwarded, net_access) {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(repo_url.to_string()),
                    certainty: Some(Certainty::Possible),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
        }
    }

    Ok(upstream_data)
}

pub fn metadata_from_itp_bug_body(
    body: &str,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut results: Vec<UpstreamDatumWithMetadata> = Vec::new();
    // Skip first few lines with bug metadata (severity, owner, etc)
    let mut line_iter = body.split_terminator('\n');
    let mut next_line = line_iter.next();

    while let Some(line) = next_line {
        if next_line.is_none() {
            return Err(ProviderError::ParseError(
                "ITP bug body ended before package name".to_string(),
            ));
        }
        next_line = line_iter.next();
        if line.trim().is_empty() {
            break;
        }
    }

    while let Some(line) = next_line {
        if next_line.is_none() {
            return Err(ProviderError::ParseError(
                "ITP bug body ended before package name".to_string(),
            ));
        }
        if !line.is_empty() {
            break;
        }
        next_line = line_iter.next();
    }

    while let Some(mut line) = next_line {
        line = line.trim_start_matches('*').trim_start();

        if line.is_empty() {
            break;
        }

        match line.split_once(':') {
            Some((key, value)) => {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "Package name" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Name(value.to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: None,
                        });
                    }
                    "Version" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Version(value.to_string()),
                            certainty: Some(Certainty::Possible),
                            origin: None,
                        });
                    }
                    "Upstream Author" if !value.is_empty() => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Author(vec![Person::from(value)]),
                            certainty: Some(Certainty::Confident),
                            origin: None,
                        });
                    }
                    "URL" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Homepage(value.to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: None,
                        });
                    }
                    "License" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::License(value.to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: None,
                        });
                    }
                    "Description" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Summary(value.to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: None,
                        });
                    }
                    _ => {
                        debug!("Unknown pseudo-header {} in ITP bug body", key);
                    }
                }
            }
            _ => {
                debug!("Ignoring non-semi-field line {}", line);
            }
        }

        next_line = line_iter.next();
    }

    let mut rest: Vec<String> = Vec::new();
    for line in line_iter {
        if line.trim() == "-- System Information:" {
            break;
        }
        rest.push(line.to_string());
    }

    results.push(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Description(rest.join("\n")),
        certainty: Some(Certainty::Likely),
        origin: None,
    });

    Ok(results)
}
