//! See https://golang.org/doc/modules/gomod-ref
use crate::{Certainty, UpstreamDatum, UpstreamDatumWithMetadata};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub fn guess_from_go_mod(path: &Path, _trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    let file = File::open(path).expect("Failed to open file");
    let reader = BufReader::new(file);
    let mut results = Vec::new();

    for line in reader.lines() {
        if let Ok(line) = line {
            if line.starts_with("module ") {
                let modname = line.trim().split_once(' ').unwrap().1;
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(modname.to_owned()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
        }
    }

    results
}
