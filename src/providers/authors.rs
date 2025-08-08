use crate::{
    Certainty, GuesserSettings, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata,
};
use std::fs::File;
use std::io::BufRead;
use std::path::Path;

/// Extracts author information from AUTHORS file
pub fn guess_from_authors(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let file = File::open(path)?;
    let reader = std::io::BufReader::new(file);

    let mut authors: Vec<Person> = Vec::new();

    for line in reader.lines().map_while(Result::ok) {
        let mut m = line.trim().to_string();
        if m.is_empty() {
            continue;
        }
        if m.starts_with("arch-tag: ") {
            continue;
        }
        if m.ends_with(':') {
            continue;
        }
        if m.starts_with("$Id") {
            continue;
        }
        if m.starts_with('*') || m.starts_with('-') {
            m = m[1..].trim().to_string();
        }
        if m.len() < 3 {
            continue;
        }
        if m.ends_with('.') {
            continue;
        }
        if m.contains(" for ") {
            let parts: Vec<&str> = m.split(" for ").collect();
            m = parts[0].to_string();
        }
        if !m.chars().next().unwrap().is_alphabetic() {
            continue;
        }
        if !m.contains('<') && line.as_bytes().starts_with(b"\t") {
            continue;
        }
        if m.contains('<') || m.matches(' ').count() < 5 {
            authors.push(Person::from(m.as_str()));
        }
    }

    Ok(vec![UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Author(authors),
        certainty: Some(Certainty::Likely),
        origin: Some(path.into()),
    }])
}
