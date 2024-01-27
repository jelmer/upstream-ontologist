//! Documentation: https://maven.apache.org/pom.html

use crate::{vcs, Certainty, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use log::{error, warn};
use std::fs::File;
use std::path::Path;

pub fn guess_from_pom_xml(
    path: &Path,
    _trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    use xmltree::Element;
    let file = File::open(path).expect("Failed to open file");

    let root = Element::parse(file)
        .map_err(|e| ProviderError::ParseError(format!("Unable to parse package.xml: {}", e)))?;

    let mut result = Vec::new();
    if root.name == "project" {
        if let Some(name_tag) = root.get_child("name") {
            if let Some(name) = name_tag.get_text() {
                if !name.contains('$') {
                    result.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Name(name.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
        } else if let Some(artifact_id_tag) = root.get_child("artifactId") {
            if let Some(artifact_id) = artifact_id_tag.get_text() {
                result.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(artifact_id.to_string()),
                    certainty: Some(Certainty::Possible),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
        }

        if let Some(description_tag) = root.get_child("description") {
            if let Some(description) = description_tag.get_text() {
                result.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(description.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
        }

        if let Some(version_tag) = root.get_child("version") {
            if let Some(version) = version_tag.get_text() {
                if !version.contains('$') {
                    result.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Version(version.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
        }

        if let Some(licenses_tag) = root.get_child("licenses") {
            for license_tag in licenses_tag
                .children
                .iter()
                .filter(|c| c.as_element().map_or(false, |e| e.name == "license"))
            {
                if let Some(license_tag) = license_tag.as_element() {
                    if let Some(name_tag) = license_tag.get_child("name") {
                        if let Some(license_name) = name_tag.get_text() {
                            result.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::License(license_name.to_string()),
                                certainty: Some(Certainty::Certain),
                                origin: Some(path.to_string_lossy().to_string()),
                            });
                        }
                    }
                }
            }
        }

        for scm_tag in root
            .children
            .iter()
            .filter(|c| c.as_element().map_or(false, |e| e.name == "scm"))
        {
            if let Some(scm_tag) = scm_tag.as_element() {
                if let Some(url_tag) = scm_tag.get_child("url") {
                    if let Some(url) = url_tag.get_text() {
                        if url.starts_with("scm:") && url.matches(':').count() >= 3 {
                            let url_parts: Vec<&str> = url.splitn(3, ':').collect();
                            let browse_url = url_parts[2];
                            if vcs::plausible_browse_url(browse_url) {
                                result.push(UpstreamDatumWithMetadata {
                                    datum: UpstreamDatum::RepositoryBrowse(browse_url.to_owned()),
                                    certainty: Some(Certainty::Certain),
                                    origin: Some(path.to_string_lossy().to_string()),
                                });
                            }
                        } else {
                            result.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::RepositoryBrowse(url.to_string()),
                                certainty: Some(Certainty::Certain),
                                origin: Some(path.to_string_lossy().to_string()),
                            });
                        }
                    }
                }

                if let Some(connection_tag) = scm_tag.get_child("connection") {
                    if let Some(connection) = connection_tag.get_text() {
                        let connection_parts: Vec<&str> = connection.splitn(3, ':').collect();
                        if connection_parts.len() == 3 && connection_parts[0] == "scm" {
                            result.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::Repository(connection_parts[2].to_owned()),
                                certainty: Some(Certainty::Certain),
                                origin: Some(path.to_string_lossy().to_string()),
                            });
                        } else {
                            warn!("Invalid format for SCM connection: {}", connection);
                        }
                    }
                }
            }
        }

        for issue_mgmt_tag in root.children.iter().filter(|c| {
            c.as_element()
                .map_or(false, |e| e.name == "issueManagement")
        }) {
            if let Some(issue_mgmt_tag) = issue_mgmt_tag.as_element() {
                if let Some(url_tag) = issue_mgmt_tag.get_child("url") {
                    if let Some(url) = url_tag.get_text() {
                        result.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::BugDatabase(url.to_string()),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.to_string_lossy().to_string()),
                        });
                    }
                }
            }
        }

        if let Some(url_tag) = root.get_child("url") {
            if let Some(url) = url_tag.get_text() {
                if !url.starts_with("scm:") {
                    result.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Homepage(url.into_owned()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
        }
    }

    Ok(result)
}
