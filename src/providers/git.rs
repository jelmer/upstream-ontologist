use crate::{Certainty, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use std::path::Path;

#[cfg(feature = "git-config")]
pub fn guess_from_git_config(
    path: &Path,
    trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let config_file =
        gix_config::File::from_path_no_includes(path.to_path_buf(), gix_config::Source::Local)
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;
    let mut results = Vec::new();

    // Check if there's a remote named "upstream"
    if let Some(remote_upstream) = config_file.string_by_key("remote.upstream.url") {
        let url = remote_upstream.to_string();
        if !url.starts_with("../") {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(url),
                certainty: Some(Certainty::Likely),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
    }

    // Check if there's a remote named "origin"
    if !trust_package {
        if let Some(remote_origin) = config_file.string_by_key("remote.origin.url") {
            let url = remote_origin.to_string();
            if !url.starts_with("../") {
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(url),
                    certainty: Some(Certainty::Possible),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
        }
    }

    Ok(results)
}
