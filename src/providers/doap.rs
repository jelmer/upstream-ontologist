//! See https://github.com/ewilderj/doap
use crate::{Certainty, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use log::error;
use std::fs::File;
use std::path::Path;

pub fn guess_from_doap(
    path: &Path,
    _trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    use xmltree::Element;
    let file = File::open(path).expect("Failed to open file");
    let doc = Element::parse(file).expect("Failed to parse XML");
    let mut root = &doc;

    let mut results: Vec<UpstreamDatumWithMetadata> = Vec::new();

    const DOAP_NAMESPACE: &str = "http://usefulinc.com/ns/doap#";
    const RDF_NAMESPACE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
    const SCHEMA_NAMESPACE: &str = "https://schema.org/";

    if root.name == "RDF" && root.namespace.as_deref() == Some(RDF_NAMESPACE) {
        for child in root.children.iter() {
            if let Some(element) = child.as_element() {
                root = element;
                break;
            }
        }
    }

    if root.name != "Project" || root.namespace.as_deref() != Some(DOAP_NAMESPACE) {
        return Err(ProviderError::ParseError(format!(
            "Doap file does not have DOAP project as root, but {}",
            root.name
        )));
    }

    fn extract_url(el: &Element) -> Option<&str> {
        el.attributes.get("resource").map(|url| url.as_str())
    }

    fn extract_lang(el: &Element) -> Option<&str> {
        el.attributes.get("lang").map(|lang| lang.as_str())
    }

    let mut screenshots: Vec<String> = Vec::new();
    let mut maintainers: Vec<Person> = Vec::new();

    for child in &root.children {
        let child = if let Some(element) = child.as_element() {
            element
        } else {
            continue;
        };
        match (child.namespace.as_deref(), child.name.as_str()) {
            (Some(DOAP_NAMESPACE), "name") => {
                if let Some(text) = &child.get_text() {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Name(text.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            (Some(DOAP_NAMESPACE), "shortname") | (Some(DOAP_NAMESPACE), "short-name") => {
                if let Some(text) = &child.get_text() {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Name(text.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            (Some(DOAP_NAMESPACE), "bug-database") => {
                if let Some(url) = extract_url(child) {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::BugDatabase(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            (Some(DOAP_NAMESPACE), "homepage") => {
                if let Some(url) = extract_url(child) {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Homepage(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            (Some(DOAP_NAMESPACE), "download-page") => {
                if let Some(url) = extract_url(child) {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Download(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            (Some(DOAP_NAMESPACE), "shortdesc") => {
                if let Some(lang) = extract_lang(child) {
                    if lang == "en" {
                        if let Some(text) = &child.get_text() {
                            results.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::Summary(text.to_string()),
                                certainty: Some(Certainty::Certain),
                                origin: Some(path.to_string_lossy().to_string()),
                            });
                        }
                    }
                }
            }
            (Some(DOAP_NAMESPACE), "description") => {
                if let Some(lang) = extract_lang(child) {
                    if lang == "en" {
                        if let Some(text) = &child.get_text() {
                            results.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::Description(text.to_string()),
                                certainty: Some(Certainty::Certain),
                                origin: Some(path.to_string_lossy().to_string()),
                            });
                        }
                    }
                }
            }
            (Some(DOAP_NAMESPACE), "license") => {
                // TODO: Handle license
            }
            (Some(DOAP_NAMESPACE), "repository") => {
                for repo in &child.children {
                    let repo = if let Some(element) = repo.as_element() {
                        element
                    } else {
                        continue;
                    };
                    match repo.name.as_str() {
                        "SVNRepository" | "GitRepository" => {
                            if let Some(repo_location) = repo.get_child("location") {
                                if let Some(repo_url) = extract_url(repo_location) {
                                    results.push(UpstreamDatumWithMetadata {
                                        datum: UpstreamDatum::Repository(repo_url.to_string()),
                                        certainty: Some(Certainty::Certain),
                                        origin: Some(path.to_string_lossy().to_string()),
                                    });
                                }
                            }
                            if let Some(web_location) = repo.get_child("browse") {
                                if let Some(web_url) = extract_url(web_location) {
                                    results.push(UpstreamDatumWithMetadata {
                                        datum: UpstreamDatum::RepositoryBrowse(web_url.to_string()),
                                        certainty: Some(Certainty::Certain),
                                        origin: Some(path.to_string_lossy().to_string()),
                                    });
                                }
                            }
                        }
                        _ => (),
                    }
                }
            }
            (Some(DOAP_NAMESPACE), "category")
            | (Some(DOAP_NAMESPACE), "programming-language")
            | (Some(DOAP_NAMESPACE), "os")
            | (Some(DOAP_NAMESPACE), "implements")
            | (Some(SCHEMA_NAMESPACE), "logo")
            | (Some(DOAP_NAMESPACE), "platform") => {
                // TODO: Handle other tags
            }
            (Some(SCHEMA_NAMESPACE), "screenshot") | (Some(DOAP_NAMESPACE), "screenshots") => {
                if let Some(url) = extract_url(child) {
                    screenshots.push(url.to_string());
                }
            }
            (Some(DOAP_NAMESPACE), "wiki") => {
                if let Some(url) = extract_url(child) {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Wiki(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            (Some(DOAP_NAMESPACE), "maintainer") => {
                for person in &child.children {
                    let person = if let Some(element) = person.as_element() {
                        element
                    } else {
                        continue;
                    };
                    if person.name != "Person" {
                        continue;
                    }
                    let name = if let Some(name_tag) = person.get_child("name") {
                        name_tag.get_text().clone()
                    } else {
                        None
                    };
                    let email = if let Some(email_tag) = person.get_child("mbox") {
                        email_tag.get_text().as_ref().cloned()
                    } else {
                        None
                    };
                    let url = if let Some(email_tag) = person.get_child("mbox") {
                        extract_url(email_tag).map(|url| url.to_string())
                    } else {
                        None
                    };
                    maintainers.push(Person {
                        name: name.map(|n| n.to_string()),
                        email: email.map(|n| n.to_string()),
                        url,
                    });
                }
            }
            (Some(DOAP_NAMESPACE), "mailing-list") => {
                if let Some(url) = extract_url(child) {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::MailingList(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            _ => {
                error!("Unknown tag {} in DOAP file", child.name);
            }
        }
    }

    if maintainers.len() == 1 {
        let maintainer = maintainers.remove(0);
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Maintainer(maintainer),
            certainty: Some(Certainty::Certain),
            origin: Some(path.to_string_lossy().to_string()),
        });
    } else {
        for maintainer in maintainers {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Maintainer(maintainer),
                certainty: Some(Certainty::Possible),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
    }

    Ok(results)
}
