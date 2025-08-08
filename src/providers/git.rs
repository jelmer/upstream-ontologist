use crate::{Certainty, GuesserSettings, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use std::path::Path;

#[cfg(feature = "git-config")]
/// Extracts upstream metadata from .git/config file
pub fn guess_from_git_config(
    path: &Path,
    settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let config_file =
        gix_config::File::from_path_no_includes(path.to_path_buf(), gix_config::Source::Local)
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;
    let mut results = Vec::new();

    // Check if there's a remote named "upstream"
    if let Some(remote_upstream) = config_file.string_by("remote", Some("upstream".into()), "url") {
        let url = remote_upstream.to_string();
        if !url.starts_with("../") {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(url),
                certainty: Some(Certainty::Likely),
                origin: Some(path.into()),
            });
        }
    }

    // Check if there's a remote named "origin"
    if !settings.trust_package {
        if let Some(remote_origin) = config_file.string_by("remote", Some("origin".into()), "url") {
            let url = remote_origin.to_string();
            if !url.starts_with("../") {
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(url),
                    certainty: Some(Certainty::Possible),
                    origin: Some(path.into()),
                });
            }
        }
    }

    Ok(results)
}
