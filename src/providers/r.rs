//! See https://r-pkgs.org/description.html

use crate::{vcs, Certainty, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use log::debug;
use std::fs::File;
use std::io::Read;
use url::Url;

#[cfg(feature = "r-description")]
pub fn guess_from_r_description(
    path: &std::path::Path,
    trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    use mailparse::MailHeaderMap;
    let mut file = File::open(path)?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)?;

    let msg =
        mailparse::parse_mail(&contents).map_err(|e| ProviderError::ParseError(e.to_string()))?;

    let headers = msg.get_headers();

    let mut results = Vec::new();

    fn parse_url_entry(entry: &str) -> Option<(&str, Option<&str>)> {
        let mut parts = entry.splitn(2, " (");
        if let Some(url) = parts.next() {
            let label = parts.next().map(|label| label.trim_end_matches(')').trim());
            Some((url.trim(), label))
        } else {
            Some((entry, None))
        }
    }

    if let Some(package) = headers.get_first_value("Package") {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(package),
            certainty: Some(Certainty::Certain),
            origin: Some("DESCRIPTION".to_string()),
        });
    }

    if let Some(repository) = headers.get_first_value("Repository") {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Archive(repository),
            certainty: Some(Certainty::Certain),
            origin: Some("DESCRIPTION".to_string()),
        });
    }

    if let Some(bug_reports) = headers.get_first_value("BugReports") {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::BugDatabase(bug_reports),
            certainty: Some(Certainty::Certain),
            origin: Some("DESCRIPTION".to_string()),
        });
    }

    if let Some(version) = headers.get_first_value("Version") {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Version(version),
            certainty: Some(Certainty::Certain),
            origin: Some("DESCRIPTION".to_string()),
        });
    }

    if let Some(license) = headers.get_first_value("License") {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::License(license),
            certainty: Some(Certainty::Certain),
            origin: Some("DESCRIPTION".to_string()),
        });
    }

    if let Some(title) = headers.get_first_value("Title") {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Summary(title),
            certainty: Some(Certainty::Certain),
            origin: Some("DESCRIPTION".to_string()),
        });
    }

    if let Some(desc) = headers
        .get_first_header("Description")
        .map(|h| h.get_value_raw())
    {
        let desc = String::from_utf8_lossy(desc);
        let lines: Vec<&str> = desc.split_inclusive('\n').collect();
        if !lines.is_empty() {
            let reflowed = format!("{}{}", lines[0], textwrap::dedent(&lines[1..].concat()));
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Description(reflowed),
                certainty: Some(Certainty::Certain),
                origin: Some("DESCRIPTION".to_string()),
            });
        }
    }

    if let Some(maintainer) = headers.get_first_value("Maintainer") {
        let person = Person::from(maintainer.as_str());
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Maintainer(person),
            certainty: Some(Certainty::Certain),
            origin: Some("DESCRIPTION".to_string()),
        });
    }

    if let Some(url) = headers.get_first_header("URL").map(|h| h.get_value_raw()) {
        let url = String::from_utf8(url.to_vec()).unwrap();
        let entries: Vec<&str> = url
            .split_terminator(|c| c == ',' || c == '\n')
            .map(str::trim)
            .collect();
        let mut urls = Vec::new();

        for entry in entries {
            if let Some((url, label)) = parse_url_entry(entry) {
                urls.push((label, url));
            }
        }

        if urls.len() == 1 {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Homepage(urls[0].1.to_string()),
                certainty: Some(Certainty::Possible),
                origin: Some("DESCRIPTION".to_string()),
            });
        }

        for (label, url) in urls {
            let url = match Url::parse(url) {
                Ok(url) => url,
                Err(_) => {
                    debug!("Invalid URL: {}", url);
                    continue;
                }
            };
            if let Some(hostname) = url.host_str() {
                if hostname == "bioconductor.org" {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Archive("Bioconductor".to_string()),
                        certainty: Some(Certainty::Confident),
                        origin: Some("DESCRIPTION".to_string()),
                    });
                }

                if label.map(str::to_lowercase).as_deref() == Some("devel")
                    || label.map(str::to_lowercase).as_deref() == Some("repository")
                {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Repository(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some("DESCRIPTION".to_string()),
                    });
                } else if label.map(str::to_lowercase).as_deref() == Some("homepage") {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Homepage(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some("DESCRIPTION".to_string()),
                    });
                } else if let Some(repo_url) = vcs::guess_repo_from_url(&url, None) {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Repository(repo_url),
                        certainty: Some(Certainty::Certain),
                        origin: Some("DESCRIPTION".to_string()),
                    });
                }
            }
        }
    }

    Ok(results)
}
