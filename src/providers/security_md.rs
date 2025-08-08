//! <https://docs.github.com/en/free-pro-team@latest/github/managing-security-vulnerabilities/adding-a-security-policy-to-your-repository>

use crate::{Certainty, GuesserSettings, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};

/// Extracts metadata from SECURITY.md files
pub fn guess_from_security_md(
    name: &str,
    path: &std::path::Path,
    _settings: &GuesserSettings,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let path = path.strip_prefix("./").unwrap_or(path);
    // TODO(jelmer): scan SECURITY.md for email addresses/URLs with instructions
    let results = vec![UpstreamDatumWithMetadata {
        datum: UpstreamDatum::SecurityMD(name.to_string()),
        certainty: Some(Certainty::Certain),
        origin: Some(path.into()),
    }];
    Ok(results)
}
