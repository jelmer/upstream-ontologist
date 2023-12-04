//! https://docs.github.com/en/free-pro-team@latest/github/\
//! managing-security-vulnerabilities/adding-a-security-policy-to-your-repository

use crate::{Certainty, UpstreamDatum, UpstreamDatumWithMetadata, ProviderError};

pub fn guess_from_security_md(
    name: &str,
    path: &std::path::Path,
    _trust_package: bool,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let path = path.strip_prefix("./").unwrap_or(path);
    let mut results = Vec::new();
    // TODO(jelmer): scan SECURITY.md for email addresses/URLs with instructions
    results.push(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::SecurityMD(name.to_string()),
        certainty: Some(Certainty::Certain),
        origin: Some(path.to_string_lossy().to_string()),
    });
    Ok(results)
}
