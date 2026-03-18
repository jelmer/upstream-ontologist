//! See <https://www.freedesktop.org/software/appstream/docs/chap-Metadata.html>

use crate::{Certainty, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use std::fs::File;
use std::path::Path;

/// Extracts upstream metadata from AppStream metainfo XML files
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
            if let Some(text) = child.get_text() {
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(text.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
        }
        if child.name == "project_license" {
            if let Some(text) = child.get_text() {
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License(text.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
        }
        if child.name == "url" {
            if let Some(urltype) = child.attributes.get("type") {
                if urltype == "homepage" {
                    if let Some(text) = child.get_text() {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Homepage(text.to_string()),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.into()),
                        });
                    }
                } else if urltype == "bugtracker" {
                    if let Some(text) = child.get_text() {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::BugDatabase(text.to_string()),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.into()),
                        });
                    }
                }
            }
        }
        if child.name == "description" {
            if let Some(text) = child.get_text() {
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Description(text.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
        }
        if child.name == "summary" {
            if let Some(text) = child.get_text() {
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(text.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
        }
        if child.name == "name" {
            if let Some(text) = child.get_text() {
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(text.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_xml(td: &tempfile::TempDir, content: &str) -> std::path::PathBuf {
        let path = td.path().join("test.metainfo.xml");
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_empty_elements() {
        let td = tempfile::tempdir().unwrap();
        let path = write_xml(
            &td,
            r#"<component>
  <id></id>
  <name></name>
  <summary></summary>
  <description></description>
</component>"#,
        );
        let result = guess_from_metainfo(&path, false).unwrap();
        assert_eq!(result, vec![]);
    }

    #[test]
    fn test_self_closing_elements() {
        let td = tempfile::tempdir().unwrap();
        let path = write_xml(
            &td,
            r#"<component>
  <id/>
  <name/>
  <url type="homepage"/>
</component>"#,
        );
        let result = guess_from_metainfo(&path, false).unwrap();
        assert_eq!(result, vec![]);
    }

    #[test]
    fn test_invalid_xml() {
        let td = tempfile::tempdir().unwrap();
        let path = write_xml(&td, "this is not xml at all");
        let result = guess_from_metainfo(&path, false);
        assert!(result.is_err());
    }
}
