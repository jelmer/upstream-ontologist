use crate::{
    Certainty, GuesserSettings, Origin, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata,
    UpstreamMetadata,
};
use lazy_regex::regex;
use serde::Deserialize;

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn guess_from_pod(
    contents: &str,
    origin: &Origin,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut by_header: HashMap<String, String> = HashMap::new();
    let mut inheader: Option<String> = None;

    for line in contents.lines() {
        if line.starts_with("=head1 ") {
            inheader = Some(line.trim_start_matches("=head1 ").to_string());
            by_header.insert(inheader.clone().unwrap().to_uppercase(), String::new());
        } else if let Some(header) = &inheader {
            if let Some(value) = by_header.get_mut(&header.to_uppercase()) {
                value.push_str(line)
            }
        }
    }

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();

    if let Some(description) = by_header.get("DESCRIPTION") {
        let mut description = description.trim_start_matches('\n').to_string();
        description = regex!(r"[FXZSCBI]\\<([^>]+)>")
            .replace_all(&description, "$1")
            .into_owned();
        description = regex!(r"L\\<([^\|]+)\|([^\\>]+)\\>")
            .replace_all(&description, "$2")
            .into_owned();
        description = regex!(r"L\\<([^\\>]+)\\>")
            .replace_all(&description, "$1")
            .into_owned();

        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Description(description),
            certainty: Some(Certainty::Certain),
            origin: Some(origin.clone()),
        });
    }

    if let Some(name) = by_header.get("NAME") {
        let lines: Vec<&str> = name.trim().lines().collect();
        if let Some(line) = lines.first() {
            if let Some((name, summary)) = line.split_once(" - ") {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(name.trim().to_string()),
                    certainty: Some(Certainty::Confident),
                    origin: Some(origin.clone()),
                });
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(summary.trim().to_string()),
                    certainty: Some(Certainty::Confident),
                    origin: Some(origin.clone()),
                });
            } else if !line.contains(' ') {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(line.trim().to_string()),
                    certainty: Some(Certainty::Confident),
                    origin: Some(origin.clone()),
                });
            }
        }
    }

    Ok(upstream_data)
}

pub fn guess_from_perl_module(
    path: &Path,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    match Command::new("perldoc").arg("-u").arg(path).output() {
        Ok(output) => guess_from_pod(
            &String::from_utf8_lossy(&output.stdout),
            &Origin::Path(path.into()),
        ),
        Err(e) => Err(ProviderError::Other(format!(
            "Error running perldoc: {}",
            e
        ))),
    }
}

pub fn guess_from_perl_dist_name(
    path: &Path,
    dist_name: &str,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mod_path = PathBuf::from(format!(
        "{}/lib/{}.pm",
        std::path::Path::new(path)
            .parent()
            .expect("parent")
            .display(),
        dist_name.replace('-', "/")
    ));

    if mod_path.exists() {
        guess_from_perl_module(mod_path.as_path())
    } else {
        Ok(Vec::new())
    }
}

#[cfg(feature = "dist-ini")]
pub fn guess_from_dist_ini(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let parser = ini::Ini::load_from_file(path)
        .map_err(|e| ProviderError::ParseError(format!("Error parsing dist.ini: {}", e)))?;

    let dist_name = parser
        .get_from::<&str>(None, "name")
        .map(|name| UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(name.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });

    let version =
        parser
            .get_from::<&str>(None, "version")
            .map(|version| UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Version(version.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });

    let summary =
        parser
            .get_from::<&str>(None, "abstract")
            .map(|summary| UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Summary(summary.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });

    let bug_database = parser
        .get_from(Some("MetaResources"), "bugtracker.web")
        .map(|bugtracker| UpstreamDatumWithMetadata {
            datum: UpstreamDatum::BugDatabase(bugtracker.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });

    let repository = parser
        .get_from(Some("MetaResources"), "repository.url")
        .map(|repository| UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository(repository.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });

    let license =
        parser
            .get_from::<&str>(None, "license")
            .map(|license| UpstreamDatumWithMetadata {
                datum: UpstreamDatum::License(license.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });

    let copyright = match (
        parser.get_from::<&str>(None, "copyright_year"),
        parser.get_from::<&str>(None, "copyright_holder"),
    ) {
        (Some(year), Some(holder)) => Some(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Copyright(format!("{} {}", year, holder)),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        }),
        _ => None,
    };

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();

    if let Some(dist_name) = dist_name {
        upstream_data.push(dist_name);
    }
    if let Some(version) = version {
        upstream_data.push(version);
    }
    if let Some(summary) = summary {
        upstream_data.push(summary);
    }
    if let Some(bug_database) = bug_database {
        upstream_data.push(bug_database);
    }
    if let Some(repository) = repository {
        upstream_data.push(repository);
    }
    if let Some(license) = license {
        upstream_data.push(license);
    }
    if let Some(copyright) = copyright {
        upstream_data.push(copyright);
    }

    if let Some(dist_name) = parser.get_from::<&str>(None, "name") {
        upstream_data.extend(guess_from_perl_dist_name(path, dist_name)?);
    }

    Ok(upstream_data)
}

pub fn guess_from_meta_json(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let data: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&contents)
        .map_err(|e| ProviderError::ParseError(format!("Error parsing META.json: {}", e)))?;

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();

    if let Some(name) = data.get("name").and_then(serde_json::Value::as_str) {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(name.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(version) = data.get("version").and_then(serde_json::Value::as_str) {
        let version = version.strip_prefix('v').unwrap_or(version);
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Version(version.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(summary) = data.get("abstract").and_then(serde_json::Value::as_str) {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Summary(summary.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(resources) = data.get("resources").and_then(serde_json::Value::as_object) {
        if let Some(bugtracker) = resources
            .get("bugtracker")
            .and_then(serde_json::Value::as_object)
        {
            if let Some(web) = bugtracker.get("web").and_then(serde_json::Value::as_str) {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::BugDatabase(web.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
                // TODO: Support resources["bugtracker"]["mailto"]
            }
        }

        if let Some(homepage) = resources
            .get("homepage")
            .and_then(serde_json::Value::as_str)
        {
            upstream_data.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Homepage(homepage.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }

        if let Some(repo) = resources
            .get("repository")
            .and_then(serde_json::Value::as_object)
        {
            if let Some(url) = repo.get("url").and_then(serde_json::Value::as_str) {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(url.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }

            if let Some(web) = repo.get("web").and_then(serde_json::Value::as_str) {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::RepositoryBrowse(web.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
        }
    }

    // Wild guess:
    if let Some(dist_name) = data.get("name").and_then(serde_json::Value::as_str) {
        upstream_data.extend(guess_from_perl_dist_name(path, dist_name)?);
    }

    Ok(upstream_data)
}

/// Guess upstream metadata from a META.yml file.
///
/// See <http://module-build.sourceforge.net/META-spec-v1.4.html> for the
/// specification of the format.
pub fn guess_from_meta_yml(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut file = File::open(path)?;

    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let data: serde_yaml::Value = serde_yaml::from_str(&contents)
        .map_err(|e| ProviderError::ParseError(format!("Error parsing META.yml: {}", e)))?;

    let mut upstream_data = Vec::new();

    if let Some(name) = data.get("name") {
        if let Some(name) = name.as_str() {
            upstream_data.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(name.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }
    }

    if let Some(license) = data.get("license") {
        if let Some(license) = license.as_str() {
            upstream_data.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::License(license.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }
    }

    if let Some(version) = data.get("version") {
        if let Some(version) = version.as_str() {
            upstream_data.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Version(version.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }
    }

    if let Some(resources) = data.get("resources") {
        if let Some(bugtracker) = resources.get("bugtracker") {
            upstream_data.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::BugDatabase(bugtracker.as_str().unwrap().to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }

        if let Some(homepage) = resources.get("homepage") {
            upstream_data.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Homepage(homepage.as_str().unwrap().to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }

        if let Some(repository) = resources.get("repository") {
            if let Some(url) = repository.get("url") {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(url.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            } else {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(repository.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
        }
    }

    // Wild guess:
    if let Some(dist_name) = data.get("name") {
        if let Some(dist_name) = dist_name.as_str() {
            upstream_data.extend(guess_from_perl_dist_name(path, dist_name)?);
        }
    }

    Ok(upstream_data)
}

pub fn guess_from_makefile_pl(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut dist_name = None;
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut results = Vec::new();
    let name_regex = regex!("name '([^'\"]+)';$");
    let repository_regex = regex!("repository '([^'\"]+)';$");

    for line in reader.lines().map_while(Result::ok) {
        if let Some(captures) = name_regex.captures(&line) {
            dist_name = Some(captures.get(1).unwrap().as_str().to_owned());
            let name = dist_name.as_ref().unwrap().to_owned();
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(name),
                certainty: Some(Certainty::Confident),
                origin: Some(path.into()),
            });
        }
        if let Some(captures) = repository_regex.captures(&line) {
            let repository = captures.get(1).unwrap().as_str().to_owned();
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(repository),
                certainty: Some(Certainty::Confident),
                origin: Some(path.into()),
            });
        }
    }

    if let Some(dist_name) = dist_name {
        results.extend(guess_from_perl_dist_name(path, &dist_name)?);
    }

    Ok(results)
}

#[derive(Deserialize)]
pub struct Module {
    pub version_numified: f64,
    pub version: String,
    pub authorized: bool,
    pub name: String,
    pub indexed: bool,
}

#[derive(Deserialize)]
pub struct Stat {
    pub uid: isize,
    pub mtime: isize,
    pub size: isize,
    pub mode: isize,
    pub gid: isize,
}

#[derive(Deserialize)]
pub struct CpanModule {
    pub maturity: String,
    pub release: String,
    pub author: String,
    pub slop: isize,
    pub download_url: url::Url,
    pub module: Vec<Module>,
    pub pod_lines: Vec<String>,
    pub version: String,
    pub deprecated: bool,
    pub level: isize,
    pub mime: String,
    pub date: String,
    pub path: String,
    pub distribution: String,
    pub pod: String,
    pub name: String,
    pub sloc: isize,
    pub stat: Stat,
    pub version_numified: f64,
    pub binary: bool,
    pub id: String,
    pub directory: bool,
    pub indexed: bool,
    pub authorized: bool,
}

impl TryFrom<CpanModule> for UpstreamMetadata {
    type Error = crate::ProviderError;

    fn try_from(value: CpanModule) -> Result<Self, Self::Error> {
        let mut metadata = UpstreamMetadata::default();

        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(value.name),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Version(value.version),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Homepage(value.download_url.to_string()),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Download(value.download_url.to_string()),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Author(vec![crate::Person::from(value.author.as_str())]),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        Ok(metadata)
    }
}

pub async fn load_cpan_data(module: &str) -> Result<Option<CpanModule>, crate::ProviderError> {
    let url = format!("https://fastapi.metacpan.org/v1/release/{}", module)
        .parse()
        .unwrap();

    let data = crate::load_json_url(&url, None).await?;

    Ok(Some(serde_json::from_value(data).unwrap()))
}

pub async fn remote_cpan_data(module: &str) -> Result<UpstreamMetadata, crate::ProviderError> {
    let data = load_cpan_data(module).await?;

    match data {
        Some(data) => data.try_into(),
        None => Ok(UpstreamMetadata::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_from_json() {
        let text = include_str!("../testdata/cpan.json");
        let cpan_module: CpanModule = serde_json::from_str(text).unwrap();

        assert_eq!("Parse-Pidl-0.02", cpan_module.release);
    }
}
