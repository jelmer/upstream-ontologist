use crate::{Certainty, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use log::error;
use std::fs::File;
use std::path::Path;

#[derive(serde::Deserialize)]
struct Pubspec {
    name: Option<String>,
    description: Option<String>,
    version: Option<String>,
    homepage: Option<String>,
    repository: Option<String>,
    documentation: Option<String>,
    issue_tracker: Option<String>,
}

pub fn guess_from_pubspec_yaml(
    path: &Path,
    _trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let file = File::open(path)?;

    let pubspec: Pubspec =
        serde_yaml::from_reader(file).map_err(|e| ProviderError::ParseError(e.to_string()))?;

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();

    if let Some(name) = pubspec.name {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(name),
            certainty: Some(Certainty::Certain),
            origin: Some("pubspec.yaml".to_string()),
        });
    }
    if let Some(description) = pubspec.description {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Description(description),
            certainty: Some(Certainty::Certain),
            origin: Some("pubspec.yaml".to_string()),
        });
    }
    if let Some(version) = pubspec.version {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Version(version),
            certainty: Some(Certainty::Certain),
            origin: Some("pubspec.yaml".to_string()),
        });
    }
    if let Some(homepage) = pubspec.homepage {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Homepage(homepage),
            certainty: Some(Certainty::Certain),
            origin: Some("pubspec.yaml".to_string()),
        });
    }
    if let Some(repository) = pubspec.repository {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository(repository),
            certainty: Some(Certainty::Certain),
            origin: Some("pubspec.yaml".to_string()),
        });
    }
    if let Some(documentation) = pubspec.documentation {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Documentation(documentation),
            certainty: Some(Certainty::Certain),
            origin: Some("pubspec.yaml".to_string()),
        });
    }
    if let Some(issue_tracker) = pubspec.issue_tracker {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::BugDatabase(issue_tracker),
            certainty: Some(Certainty::Certain),
            origin: Some("pubspec.yaml".to_string()),
        });
    }

    Ok(upstream_data)
}
