use crate::xmlparse_simplify_namespaces;
use crate::{Certainty, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use log::error;
use std::path::Path;

pub fn guess_from_package_xml(
    path: &Path,
    _trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    use xmltree::{Element, XMLNode};
    const NAMESPACES: &[&str] = &[
        "http://pear.php.net/dtd/package-2.0",
        "http://pear.php.net/dtd/package-2.1",
    ];

    let root = xmlparse_simplify_namespaces(path, NAMESPACES)
        .ok_or_else(|| ProviderError::ParseError("Unable to parse package.xml".to_string()))?;

    assert_eq!(root.name, "package", "root tag is {:?}", root.name);

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();
    let mut leads: Vec<&Element> = Vec::new();
    let mut maintainers: Vec<&Element> = Vec::new();
    let mut authors: Vec<&Element> = Vec::new();

    for child_element in &root.children {
        if let XMLNode::Element(ref element) = child_element {
            match element.name.as_str() {
                "name" => {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Name(element.get_text().unwrap().to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some("package.xml".to_string()),
                    });
                }
                "summary" => {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Summary(element.get_text().unwrap().to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some("package.xml".to_string()),
                    });
                }
                "description" => {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Description(element.get_text().unwrap().to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some("package.xml".to_string()),
                    });
                }
                "version" => {
                    if let Some(release_tag) = element.get_child("release") {
                        upstream_data.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Version(
                                release_tag.get_text().unwrap().to_string(),
                            ),
                            certainty: Some(Certainty::Certain),
                            origin: Some("package.xml".to_string()),
                        });
                    }
                }
                "license" => {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::License(element.get_text().unwrap().to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some("package.xml".to_string()),
                    });
                }
                "url" => {
                    if let Some(url_type) = element.attributes.get("type") {
                        match url_type.as_str() {
                            "repository" => {
                                upstream_data.push(UpstreamDatumWithMetadata {
                                    datum: UpstreamDatum::Repository(
                                        element.get_text().unwrap().to_string(),
                                    ),
                                    certainty: Some(Certainty::Certain),
                                    origin: Some("package.xml".to_string()),
                                });
                            }
                            "bugtracker" => {
                                upstream_data.push(UpstreamDatumWithMetadata {
                                    datum: UpstreamDatum::BugDatabase(
                                        element.get_text().unwrap().to_string(),
                                    ),
                                    certainty: Some(Certainty::Certain),
                                    origin: Some("package.xml".to_string()),
                                });
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
        let name_el = lead_element.get_child("name").unwrap().get_text();
        let email_el = lead_element
            .get_child("email")
            .map(|s| s.get_text().unwrap());
        let active_el = lead_element
            .get_child("active")
            .map(|s| s.get_text().unwrap());
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
            origin: Some("package.xml".to_string()),
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
            origin: Some("package.xml".to_string()),
        });
    }

    if !authors.is_empty() {
        let persons = authors
            .iter()
            .map(|author_element| {
                let name_el = author_element.get_text().unwrap().into_owned();
                let email_el = author_element.attributes.get("email");
                Person {
                    name: Some(name_el),
                    email: email_el.map(|s| s.to_string()),
                    ..Default::default()
                }
            })
            .collect();
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Author(persons),
            certainty: Some(Certainty::Confident),
            origin: Some("package.xml".to_string()),
        });
    }

    Ok(upstream_data)
}
