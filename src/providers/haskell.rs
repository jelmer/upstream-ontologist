use crate::{Certainty, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub fn guess_from_cabal_lines(
    lines: impl Iterator<Item = String>,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut repo_url = None;
    let mut repo_branch = None;
    let mut repo_subpath = None;

    let mut section = None;
    let mut results = Vec::new();

    for line in lines {
        if line.trim_start().starts_with("--") {
            // Comment
            continue;
        }
        if line.trim().is_empty() {
            section = None;
            continue;
        }
        let line_parts: Vec<&str> = line.splitn(2, ':').collect();
        if line_parts.len() != 2 {
            if !line.starts_with(' ') {
                section = Some(line.trim().to_lowercase());
            }
            continue;
        }
        let field = line_parts[0].trim().to_lowercase();
        let value = line_parts[1].trim();

        if !field.starts_with(' ') {
            match field.as_str() {
                "homepage" => results.push((
                    UpstreamDatum::Homepage(value.to_owned()),
                    Certainty::Certain,
                )),
                "bug-reports" => results.push((
                    UpstreamDatum::BugDatabase(value.to_owned()),
                    Certainty::Certain,
                )),
                "name" => results.push((UpstreamDatum::Name(value.to_owned()), Certainty::Certain)),
                "maintainer" => results.push((
                    UpstreamDatum::Maintainer(Person::from(value)),
                    Certainty::Certain,
                )),
                "copyright" => results.push((
                    UpstreamDatum::Copyright(value.to_owned()),
                    Certainty::Certain,
                )),
                "license" => {
                    results.push((UpstreamDatum::License(value.to_owned()), Certainty::Certain))
                }
                "author" => results.push((
                    UpstreamDatum::Author(vec![Person::from(value)]),
                    Certainty::Certain,
                )),
                _ => {}
            }
        } else {
            let field = field.trim();
            if section == Some("source-repository head".to_lowercase()) {
                match field {
                    "location" => repo_url = Some(value.to_owned()),
                    "branch" => repo_branch = Some(value.to_owned()),
                    "subdir" => repo_subpath = Some(value.to_owned()),
                    _ => {}
                }
            }
        }
    }

    if let (Some(repo_url), Some(repo_branch), Some(repo_subpath)) =
        (repo_url, repo_branch, repo_subpath)
    {
        results.push((
            UpstreamDatum::Repository(crate::vcs::unsplit_vcs_url(
                &repo_url,
                Some(&repo_branch),
                Some(&repo_subpath),
            )),
            Certainty::Certain,
        ));
    }

    Ok(results
        .into_iter()
        .map(|(datum, certainty)| UpstreamDatumWithMetadata {
            datum,
            certainty: Some(certainty),
            origin: None,
        })
        .collect())
}

pub fn guess_from_cabal(
    path: &Path,
    _trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    guess_from_cabal_lines(
        reader
            .lines()
            .map(|line| line.expect("Failed to read line")),
    )
}
