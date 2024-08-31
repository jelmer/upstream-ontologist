//! See https://golang.org/doc/modules/gomod-ref
use crate::{
    Certainty, GuesserSettings, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata,
    UpstreamMetadata,
};
use log::debug;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub fn guess_from_go_mod(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let file = File::open(path).expect("Failed to open file");
    let reader = BufReader::new(file);
    let mut results = Vec::new();

    for line in reader.lines().map_while(Result::ok) {
        if line.starts_with("module ") {
            let modname = match line.trim().split_once(' ') {
                Some((_, modname)) => modname,
                None => {
                    debug!("Failed to parse module name from line: {}", line);
                    continue;
                }
            };
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(modname.to_owned()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }
    }

    Ok(results)
}

pub fn remote_go_metadata(package: &str) -> Result<UpstreamMetadata, ProviderError> {
    let mut ret = UpstreamMetadata::default();
    if package.starts_with("github.com/") {
        ret.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::GoImportPath(package.to_string()),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        let parts: Vec<&str> = package.split('/').collect();
        ret.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository(format!("https://{}", parts[..3].join("/"))),
            certainty: Some(Certainty::Certain),
            origin: None,
        });
    }
    Ok(ret)
}
