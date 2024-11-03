use crate::xmlparse_simplify_namespaces;
use crate::{Certainty, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use std::path::Path;

// Documentation: https://docs.microsoft.com/en-us/nuget/reference/nuspec
pub async fn guess_from_nuspec(
    path: &Path,
    _trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    const NAMESPACES: &[&str] = &["http://schemas.microsoft.com/packaging/2010/07/nuspec.xsd"];
    // XML parsing and other logic
    let root = match xmlparse_simplify_namespaces(path, NAMESPACES) {
        Some(root) => root,
        None => {
            return Err(crate::ProviderError::ParseError(
                "Unable to parse nuspec".to_string(),
            ));
        }
    };

    assert_eq!(root.name, "package", "root tag is {}", root.name);
    let metadata = root.get_child("metadata");
    if metadata.is_none() {
        return Err(ProviderError::ParseError(
            "Unable to find metadata tag".to_string(),
        ));
    }
    let metadata = metadata.unwrap();

    let mut result = Vec::new();

    if let Some(version_tag) = metadata.get_child("version") {
        if let Some(version) = version_tag.get_text() {
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Version(version.into_owned()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }
    }

    if let Some(description_tag) = metadata.get_child("description") {
        if let Some(description) = description_tag.get_text() {
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Description(description.into_owned()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }
    }

    if let Some(authors_tag) = metadata.get_child("authors") {
        if let Some(authors) = authors_tag.get_text() {
            let authors = authors.split(',').map(Person::from).collect();
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Author(authors),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }
    }

    if let Some(project_url_tag) = metadata.get_child("projectUrl") {
        if let Some(project_url) = project_url_tag.get_text() {
            let repo_url =
                crate::vcs::guess_repo_from_url(&url::Url::parse(&project_url).unwrap(), None)
                    .await;
            if let Some(repo_url) = repo_url {
                result.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(repo_url),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.into()),
                });
            }
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Homepage(project_url.into_owned()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }
    }

    if let Some(license_tag) = metadata.get_child("license") {
        if let Some(license) = license_tag.get_text() {
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::License(license.into_owned()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }
    }

    if let Some(copyright_tag) = metadata.get_child("copyright") {
        if let Some(copyright) = copyright_tag.get_text() {
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Copyright(copyright.into_owned()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }
    }

    if let Some(title_tag) = metadata.get_child("title") {
        if let Some(title) = title_tag.get_text() {
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(title.into_owned()),
                certainty: Some(Certainty::Likely),
                origin: Some(path.into()),
            });
        }
    }

    if let Some(summary_tag) = metadata.get_child("summary") {
        if let Some(summary) = summary_tag.get_text() {
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Summary(summary.into_owned()),
                certainty: Some(Certainty::Likely),
                origin: Some(path.into()),
            });
        }
    }

    if let Some(repository_tag) = metadata.get_child("repository") {
        if let Some(repo_url) = repository_tag.attributes.get("url") {
            let branch = repository_tag.attributes.get("branch");
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(crate::vcs::unsplit_vcs_url(
                    &crate::vcs::VcsLocation {
                        url: repo_url.parse().unwrap(),
                        branch: branch.cloned(),
                        subpath: None,
                    },
                )),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }
    }

    Ok(result)
}
