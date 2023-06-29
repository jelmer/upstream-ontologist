use crate::{Certainty, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use std::path::Path;

pub fn guess_from_package_yaml(
    path: &Path,
    trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let reader = std::fs::File::open(path)?;
    let data: serde_yaml::Value =
        serde_yaml::from_reader(reader).map_err(|e| ProviderError::ParseError(e.to_string()))?;

    let mut ret = Vec::new();

    if let Some(name) = data.get("name") {
        if let Some(name) = name.as_str() {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(name.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.display().to_string()),
            });
        }
    }

    if let Some(version) = data.get("version") {
        if let Some(version) = version.as_str() {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Version(version.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.display().to_string()),
            });
        }
    }

    if let Some(authors) = data.get("author") {
        if let Some(author) = authors.as_str() {
            let authors = author.split(',').collect::<Vec<_>>();
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Author(authors.into_iter().map(Person::from).collect()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.display().to_string()),
            });
        }
    }

    if let Some(maintainers) = data.get("maintainer") {
        if let Some(maintainer) = maintainers.as_str() {
            let maintainers = maintainer.split(',').collect::<Vec<_>>();
            let mut maintainers = maintainers
                .into_iter()
                .map(Person::from)
                .collect::<Vec<_>>();
            if let Some(maintainer) = maintainers.pop() {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Maintainer(maintainer),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.display().to_string()),
                });
            }
        }
    }

    if let Some(homepage) = data.get("homepage") {
        if let Some(homepage) = homepage.as_str() {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Homepage(homepage.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.display().to_string()),
            });
        }
    }

    if let Some(description) = data.get("description") {
        if let Some(description) = description.as_str() {
            if !description.starts_with("Please see the README") {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Description(description.to_string()),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.display().to_string()),
                });
            }
        }
    }

    if let Some(synopsis) = data.get("synopsis") {
        if let Some(synopsis) = synopsis.as_str() {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Summary(synopsis.to_string()),
                certainty: Some(Certainty::Confident),
                origin: Some(path.display().to_string()),
            });
        }
    }

    if let Some(license) = data.get("license") {
        if let Some(license) = license.as_str() {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::License(license.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.display().to_string()),
            });
        }
    }

    if let Some(github) = data.get("github") {
        if let Some(github) = github.as_str() {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(format!("https://github.com/{}", github)),
                certainty: Some(Certainty::Certain),
                origin: Some(path.display().to_string()),
            });
        }
    }

    if let Some(repository) = data.get("repository") {
        if let Some(repository) = repository.as_str() {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(repository.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.display().to_string()),
            });
        }
    }

    Ok(ret)
}
