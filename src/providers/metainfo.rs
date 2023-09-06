//! See https://www.freedesktop.org/software/appstream/docs/chap-Metadata.html

use crate::{Certainty, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use std::fs::File;
use std::path::Path;

pub fn guess_from_metainfo(
    path: &Path,
    _trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    use xmltree::Element;
    let file = File::open(path)?;
    let root = Element::parse(file).map_err(|e| ProviderError::ParseError(e.to_string()))?;

    let mut results: Vec<UpstreamDatumWithMetadata> = Vec::new();

    for child in root.children {
        let child = if let Some(element) = child.as_element() {
            element
        } else {
            continue;
        };
        if child.name == "id" {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(child.get_text().unwrap().to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
        if child.name == "project_license" {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::License(child.get_text().unwrap().to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
        if child.name == "url" {
            if let Some(urltype) = child.attributes.get("type") {
                if urltype == "homepage" {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Homepage(child.get_text().unwrap().to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                } else if urltype == "bugtracker" {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::BugDatabase(child.get_text().unwrap().to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
        }
        if child.name == "description" {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Description(child.get_text().unwrap().to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
        if child.name == "summary" {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Summary(child.get_text().unwrap().to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
        if child.name == "name" {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(child.get_text().unwrap().to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
    }

    Ok(results)
}
