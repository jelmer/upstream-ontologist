use crate::{Certainty, GuesserSettings, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use lazy_regex::regex;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Extracts metadata from Waf wscript files
pub fn guess_from_wscript(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut results = Vec::new();
    let appname_regex = regex!("APPNAME = [\'\"](.*)[\'\"]");
    let version_regex = regex!("VERSION = [\'\"](.*)[\'\"]");

    for line in reader.lines().map_while(Result::ok) {
        if let Some(captures) = appname_regex.captures(&line) {
            let name = captures.get(1).unwrap().as_str().to_owned();
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(name),
                certainty: Some(Certainty::Confident),
                origin: Some(path.into()),
            });
        }
        if let Some(captures) = version_regex.captures(&line) {
            let version = captures.get(1).unwrap().as_str().to_owned();
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Version(version),
                certainty: Some(Certainty::Confident),
                origin: Some(path.into()),
            });
        }
    }

    Ok(results)
}
