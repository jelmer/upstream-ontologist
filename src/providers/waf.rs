use crate::{Certainty, UpstreamDatum, UpstreamDatumWithMetadata};
use regex::Regex;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub fn guess_from_wscript(path: &Path, _trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    let file = File::open(path).expect("Failed to open file");
    let reader = BufReader::new(file);
    let mut results = Vec::new();
    let appname_regex = Regex::new("APPNAME = [\'\"](.*)[\'\"]").unwrap();
    let version_regex = Regex::new("VERSION = [\'\"](.*)[\'\"]").unwrap();

    for line in reader.lines() {
        if let Ok(line) = line {
            if let Some(captures) = appname_regex.captures(&line) {
                let name = captures.get(1).unwrap().as_str().to_owned();
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(name),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            if let Some(captures) = version_regex.captures(&line) {
                let version = captures.get(1).unwrap().as_str().to_owned();
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(version),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
        }
    }

    results
}
