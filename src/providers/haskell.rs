use crate::{
    Certainty, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata, UpstreamMetadata,
};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub fn parse_cabal_lines(
    lines: impl Iterator<Item = String>,
) -> Vec<(Option<String>, String, String)> {
    let mut ret = Vec::new();
    let mut section = None;
    for line in lines {
        if line.trim_start().starts_with("--") {
            // Comment
            continue;
        }
        // Empty line
        if line.trim().is_empty() {
            section = None;
            continue;
        }

        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field.to_lowercase(), value.trim()),
            None => {
                if !line.starts_with(' ') {
                    section = Some(line.trim().to_lowercase());
                } else {
                    log::debug!("Failed to parse line: {}", line);
                }
                continue;
            }
        };

        if section.is_none() && !field.starts_with(' ') {
            ret.push((None, field.trim().to_string(), value.to_owned()));
        } else if field.starts_with(' ') {
            ret.push((
                section.clone(),
                field.trim().to_lowercase(),
                value.to_owned(),
            ));
        } else {
            log::debug!("Invalid field {}", field);
        }
    }
    ret
}

pub fn guess_from_cabal_lines(
    lines: impl Iterator<Item = String>,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut repo_url = None;
    let mut repo_branch = None;
    let mut repo_subpath = None;

    let mut results = Vec::new();

    for (section, key, value) in parse_cabal_lines(lines) {
        match (section.as_deref(), key.as_str()) {
            (None, "homepage") => results.push((
                UpstreamDatum::Homepage(value.to_owned()),
                Certainty::Certain,
            )),
            (None, "bug-reports") => results.push((
                UpstreamDatum::BugDatabase(value.to_owned()),
                Certainty::Certain,
            )),
            (None, "name") => {
                results.push((UpstreamDatum::Name(value.to_owned()), Certainty::Certain))
            }
            (None, "maintainer") => results.push((
                UpstreamDatum::Maintainer(Person::from(value.as_str())),
                Certainty::Certain,
            )),
            (None, "copyright") => results.push((
                UpstreamDatum::Copyright(value.to_owned()),
                Certainty::Certain,
            )),
            (None, "license") => {
                results.push((UpstreamDatum::License(value.to_owned()), Certainty::Certain))
            }
            (None, "author") => results.push((
                UpstreamDatum::Author(vec![Person::from(value.as_str())]),
                Certainty::Certain,
            )),
            (None, "synopsis") => {
                results.push((UpstreamDatum::Summary(value.to_owned()), Certainty::Certain))
            }
            (None, "cabal-version") => {}
            (None, "build-depends") => {}
            (None, "build-type") => {}
            (Some("source-repository head"), "location") => repo_url = Some(value.to_owned()),
            (Some("source-repository head"), "branch") => repo_branch = Some(value.to_owned()),
            (Some("source-repository head"), "subdir") => repo_subpath = Some(value.to_owned()),
            (s, _) if s.is_some() && s.unwrap().starts_with("executable ") => {}
            _ => {
                log::debug!("Unknown field {:?} in section {:?}", key, section);
            }
        }
    }

    if let Some(repo_url) = repo_url {
        results.push((
            UpstreamDatum::Repository(crate::vcs::unsplit_vcs_url(&crate::vcs::VcsLocation {
                url: repo_url.parse().unwrap(),
                branch: repo_branch,
                subpath: repo_subpath,
            })),
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

pub fn remote_hackage_data(package: &str) -> Result<UpstreamMetadata, ProviderError> {
    let mut ret = UpstreamMetadata::new();
    for datum in guess_from_hackage(package)? {
        ret.insert(datum);
    }
    Ok(ret)
}

pub fn guess_from_hackage(
    package: &str,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(crate::USER_AGENT)
        .build()
        .unwrap();

    let url: url::Url = format!(
        "https://hackage.haskell.org/package/{}/{}.cabal",
        package, package
    )
    .parse()
    .unwrap();

    match client.get(url).send() {
        Ok(response) => {
            let reader = BufReader::new(response);
            guess_from_cabal_lines(
                reader
                    .lines()
                    .map(|line| line.expect("Failed to read line")),
            )
        }
        Err(e) => match e.status() {
            Some(reqwest::StatusCode::NOT_FOUND) => {
                log::warn!("Package {} not found on Hackage", package);
                Ok(Vec::new())
            }
            _ => {
                log::warn!("Failed to fetch package {} from Hackage: {}", package, e);
                Err(ProviderError::Other(format!(
                    "Failed to fetch package {} from Hackage: {}",
                    package, e
                )))
            }
        },
    }
}

pub struct Hackage;

impl Default for Hackage {
    fn default() -> Self {
        Self::new()
    }
}

impl Hackage {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl crate::ThirdPartyRepository for Hackage {
    fn name(&self) -> &'static str {
        "Hackage"
    }

    fn max_supported_certainty(&self) -> Certainty {
        Certainty::Certain
    }

    fn supported_fields(&self) -> &'static [&'static str] {
        &[
            "Homepage",
            "Name",
            "Repository",
            "Maintainer",
            "Copyright",
            "License",
            "Bug-Database",
        ][..]
    }

    async fn guess_metadata(&self, name: &str) -> Result<Vec<UpstreamDatum>, ProviderError> {
        Ok(guess_from_hackage(name)?
            .into_iter()
            .map(|v| v.datum)
            .collect())
    }
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    #[test]
    fn test_parse_cabal_lines() {
        let lines = r#"Name:          foo
Version:    0.0
License: BSD3
Author: John Doe
Maintainer: John Doe <joe@example.com>
Cabal-Version: >= 1.10
Homepage: https://example.com

Executable program1
  Build-Depends:  HUnit
  Main-Is:       Main.hs

source-repository head
  type: git
  location: https://github.com/example/blah
"#;
        let parsed = parse_cabal_lines(lines.lines().map(|s| s.to_owned()));

        assert_eq!(
            parsed,
            vec![
                (None, "name".to_owned(), "foo".to_owned()),
                (None, "version".to_owned(), "0.0".to_owned()),
                (None, "license".to_owned(), "BSD3".to_owned()),
                (None, "author".to_owned(), "John Doe".to_owned()),
                (
                    None,
                    "maintainer".to_owned(),
                    "John Doe <joe@example.com>".to_owned()
                ),
                (None, "cabal-version".to_owned(), ">= 1.10".to_owned()),
                (
                    None,
                    "homepage".to_owned(),
                    "https://example.com".to_owned()
                ),
                (
                    Some("executable program1".to_owned()),
                    "build-depends".to_owned(),
                    "HUnit".to_owned()
                ),
                (
                    Some("executable program1".to_owned()),
                    "main-is".to_owned(),
                    "Main.hs".to_owned()
                ),
                (
                    Some("source-repository head".to_owned()),
                    "type".to_owned(),
                    "git".to_owned()
                ),
                (
                    Some("source-repository head".to_owned()),
                    "location".to_owned(),
                    "https://github.com/example/blah".to_owned()
                )
            ]
        );
    }
}
