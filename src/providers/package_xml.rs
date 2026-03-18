use crate::xmlparse_simplify_namespaces;
use crate::{
    Certainty, GuesserSettings, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata,
};
use log::error;
use std::path::Path;

/// Extracts upstream metadata from PEAR package.xml file
pub fn guess_from_package_xml(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    use xmltree::{Element, XMLNode};
    const NAMESPACES: &[&str] = &[
        "http://pear.php.net/dtd/package-2.0",
        "http://pear.php.net/dtd/package-2.1",
    ];

    let root = xmlparse_simplify_namespaces(path, NAMESPACES)
        .ok_or_else(|| ProviderError::ParseError("Unable to parse package.xml".to_string()))?;

    if root.name != "package" {
        return Err(ProviderError::ParseError(format!(
            "Expected 'package' root tag, got {:?}",
            root.name
        )));
    }

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();
    let mut leads: Vec<&Element> = Vec::new();
    let mut maintainers: Vec<&Element> = Vec::new();
    let mut authors: Vec<&Element> = Vec::new();

    for child_element in &root.children {
        if let XMLNode::Element(ref element) = child_element {
            match element.name.as_str() {
                "name" => {
                    if let Some(text) = element.get_text() {
                        upstream_data.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Name(text.to_string()),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.into()),
                        });
                    }
                }
                "summary" => {
                    if let Some(text) = element.get_text() {
                        upstream_data.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Summary(text.to_string()),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.into()),
                        });
                    }
                }
                "description" => {
                    if let Some(text) = element.get_text() {
                        upstream_data.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Description(text.to_string()),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.into()),
                        });
                    }
                }
                "version" => {
                    if let Some(release_tag) = element.get_child("release") {
                        if let Some(text) = release_tag.get_text() {
                            upstream_data.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::Version(text.to_string()),
                                certainty: Some(Certainty::Certain),
                                origin: Some(path.into()),
                            });
                        }
                    }
                }
                "license" => {
                    if let Some(text) = element.get_text() {
                        upstream_data.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::License(text.to_string()),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.into()),
                        });
                    }
                }
                "url" => {
                    if let Some(url_type) = element.attributes.get("type") {
                        match url_type.as_str() {
                            "repository" => {
                                if let Some(text) = element.get_text() {
                                    upstream_data.push(UpstreamDatumWithMetadata {
                                        datum: UpstreamDatum::Repository(text.to_string()),
                                        certainty: Some(Certainty::Certain),
                                        origin: Some(path.into()),
                                    });
                                }
                            }
                            "bugtracker" => {
                                if let Some(text) = element.get_text() {
                                    upstream_data.push(UpstreamDatumWithMetadata {
                                        datum: UpstreamDatum::BugDatabase(text.to_string()),
                                        certainty: Some(Certainty::Certain),
                                        origin: Some(path.into()),
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
                "lead" => {
                    leads.push(element);
                }
                "maintainer" => {
                    maintainers.push(element);
                }
                "author" => {
                    authors.push(element);
                }
                "stability" | "dependencies" | "providesextension" | "extsrcrelease"
                | "channel" | "notes" | "contents" | "date" | "time" | "depend" | "exec_depend"
                | "buildtool_depend" => {
                    // Do nothing, skip these fields
                }
                _ => {
                    error!("Unknown package.xml tag {}", element.name);
                }
            }
        }
    }

    for lead_element in leads.iter().take(1) {
        let name_el = lead_element.get_child("name").and_then(|s| s.get_text());
        let email_el = lead_element.get_child("email").and_then(|s| s.get_text());
        let active_el = lead_element.get_child("active").and_then(|s| s.get_text());
        if let Some(active_el) = active_el {
            if active_el != "yes" {
                continue;
            }
        }
        let person = Person {
            name: name_el.map(|s| s.to_string()),
            email: email_el.map(|s| s.to_string()),
            ..Default::default()
        };
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Maintainer(person),
            certainty: Some(Certainty::Confident),
            origin: Some(path.into()),
        });
    }

    if maintainers.len() == 1 {
        let maintainer_element = maintainers[0];
        let name_el = maintainer_element.get_text().map(|s| s.into_owned());
        let email_el = maintainer_element.attributes.get("email");
        let person = Person {
            name: name_el,
            email: email_el.map(|s| s.to_string()),
            ..Default::default()
        };
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Maintainer(person),
            certainty: Some(Certainty::Confident),
            origin: Some(path.into()),
        });
    }

    if !authors.is_empty() {
        let persons = authors
            .iter()
            .filter_map(|author_element| {
                let name_el = author_element.get_text()?.into_owned();
                let email_el = author_element.attributes.get("email");
                Some(Person {
                    name: Some(name_el),
                    email: email_el.map(|s| s.to_string()),
                    ..Default::default()
                })
            })
            .collect::<Vec<_>>();
        if !persons.is_empty() {
            upstream_data.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Author(persons),
                certainty: Some(Certainty::Confident),
                origin: Some(path.into()),
            });
        }
    }

    Ok(upstream_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_xml(td: &tempfile::TempDir, content: &str) -> std::path::PathBuf {
        let path = td.path().join("package.xml");
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_wrong_root_element() {
        let td = tempfile::tempdir().unwrap();
        let path = write_xml(&td, r#"<project><name>foo</name></project>"#);
        let result = guess_from_package_xml(&path, &GuesserSettings::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_text_elements() {
        let td = tempfile::tempdir().unwrap();
        let path = write_xml(
            &td,
            r#"<package>
  <name/>
  <summary/>
  <license/>
  <description/>
</package>"#,
        );
        let result = guess_from_package_xml(&path, &GuesserSettings::default()).unwrap();
        assert_eq!(result, vec![]);
    }

    #[test]
    fn test_lead_without_name_child() {
        let td = tempfile::tempdir().unwrap();
        let path = write_xml(
            &td,
            r#"<package>
  <lead>
    <active>yes</active>
  </lead>
</package>"#,
        );
        let result = guess_from_package_xml(&path, &GuesserSettings::default()).unwrap();
        // Should produce a Maintainer with name=None, not panic
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0].datum,
            UpstreamDatum::Maintainer(p) if p.name.is_none()
        ));
    }

    #[test]
    fn test_author_without_text() {
        let td = tempfile::tempdir().unwrap();
        let path = write_xml(
            &td,
            r#"<package>
  <author email="test@example.com"/>
</package>"#,
        );
        let result = guess_from_package_xml(&path, &GuesserSettings::default()).unwrap();
        // Author element with no text should be skipped
        assert_eq!(result, vec![]);
    }
}
