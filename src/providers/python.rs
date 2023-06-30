use crate::{py_to_upstream_datum, py_to_upstream_datum_with_metadata};
use crate::{vcs, Certainty, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use log::{debug, warn};
use pyo3::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

pub fn guess_from_pkg_info(
    path: &Path,
    trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let contents = std::fs::read(path)?;
    let dist = python_pkginfo::Metadata::parse(contents.as_slice()).map_err(|e| {
        ProviderError::ParseError(format!("Failed to parse python package metadata: {}", e))
    })?;

    let mut ret = vec![];

    ret.push(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name(dist.name),
        certainty: Some(Certainty::Certain),
        origin: Some(path.display().to_string()),
    });

    ret.push(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Version(dist.version),
        certainty: Some(Certainty::Certain),
        origin: Some(path.display().to_string()),
    });

    if let Some(homepage) = dist.home_page {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Homepage(homepage),
            certainty: Some(Certainty::Certain),
            origin: Some(path.display().to_string()),
        });
    }

    if let Some(summary) = dist.summary {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Summary(summary),
            certainty: Some(Certainty::Certain),
            origin: Some(path.display().to_string()),
        });
    }

    if let Some(description) = dist.description {
        ret.extend(parse_python_long_description(
            description.as_str(),
            dist.description_content_type.as_deref(),
        )?);
    }

    ret.extend(parse_python_project_urls(
        dist.project_urls
            .iter()
            .map(|k| k.split_once(", ").unwrap())
            .map(|(k, v)| (k.to_string(), v.to_string())),
    ));

    if dist.author.is_some() || dist.author_email.is_some() {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Author(vec![Person {
                name: dist.author,
                email: dist.author_email,
                url: None,
            }]),

            certainty: Some(Certainty::Certain),
            origin: Some(path.display().to_string()),
        });
    }

    if dist.maintainer.is_some() || dist.maintainer_email.is_some() {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Maintainer(Person {
                name: dist.maintainer,
                email: dist.maintainer_email,
                url: None,
            }),
            certainty: Some(Certainty::Certain),
            origin: Some(path.display().to_string()),
        });
    }

    if let Some(license) = dist.license {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::License(license),
            certainty: Some(Certainty::Certain),
            origin: Some(path.display().to_string()),
        });
    }

    if let Some(keywords) = dist.keywords {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Keywords(keywords.split(", ").map(|s| s.to_string()).collect()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.display().to_string()),
        });
    }

    if let Some(download_url) = dist.download_url {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Download(download_url),
            certainty: Some(Certainty::Certain),
            origin: Some(path.display().to_string()),
        });
    }

    Ok(ret)
}

pub fn guess_from_pyproject_toml(
    path: &Path,
    trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let content = std::fs::read_to_string(path)?;
    let mut ret = Vec::new();

    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct PyProjectToml {
        #[serde(flatten)]
        inner: pyproject_toml::PyProjectToml,
        tool: Option<Tool>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(rename_all = "kebab-case")]
    pub struct Tool {
        poetry: Option<ToolPoetry>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(rename_all = "kebab-case")]
    pub struct ToolPoetry {
        version: Option<String>,
        description: Option<String>,
        license: Option<String>,
        repository: Option<String>,
        name: String,
        urls: Option<HashMap<String, String>>,
        keywords: Option<Vec<String>>,
        authors: Option<Vec<String>>,
        homepage: Option<String>,
        documentation: Option<String>,
    }

    impl PyProjectToml {
        pub fn new(content: &str) -> Result<Self, toml::de::Error> {
            toml::from_str(content)
        }
    }

    let pyproject: PyProjectToml =
        toml::from_str(content.as_str()).map_err(|e| ProviderError::ParseError(e.to_string()))?;

    if let Some(inner_project) = pyproject.inner.project {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(inner_project.name),
            certainty: Some(Certainty::Certain),
            origin: Some(path.display().to_string()),
        });

        if let Some(version) = inner_project.version {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Version(version.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.display().to_string()),
            });
        }

        if let Some(license) = inner_project.license {
            match license {
                pyproject_toml::License::String(license) => {
                    ret.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::License(license),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.display().to_string()),
                    });
                }
                _ => {}
            }
        }

        fn contact_to_person(contact: &pyproject_toml::Contact) -> Person {
            Person {
                name: contact.name.clone(),
                email: contact.email.clone(),
                url: None,
            }
        }

        if let Some(authors) = inner_project.authors {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Author(authors.iter().map(contact_to_person).collect()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.display().to_string()),
            });
        }

        if let Some(maintainers) = inner_project.maintainers {
            let maintainers: Vec<_> = maintainers.iter().map(contact_to_person).collect();
            let certainty = if maintainers.len() == 1 {
                Certainty::Certain
            } else {
                Certainty::Possible
            };
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Maintainer(maintainers[0].clone()),
                certainty: Some(certainty),
                origin: Some(path.display().to_string()),
            });
        }

        if let Some(keywords) = inner_project.keywords {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Keywords(keywords),
                certainty: Some(Certainty::Certain),
                origin: Some(path.display().to_string()),
            });
        }

        if let Some(urls) = inner_project.urls {
            ret.extend(parse_python_project_urls(urls.into_iter()));
        }
    }

    if let Some(tool) = pyproject.tool {
        if let Some(poetry) = tool.poetry {
            if let Some(version) = poetry.version {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(version),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.display().to_string()),
                });
            }

            if let Some(description) = poetry.description {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(description),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.display().to_string()),
                });
            }

            if let Some(license) = poetry.license {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License(license),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.display().to_string()),
                });
            }

            if let Some(repository) = poetry.repository {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(repository),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.display().to_string()),
                });
            }

            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(poetry.name.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.display().to_string()),
            });

            if let Some(urls) = poetry.urls {
                ret.extend(parse_python_project_urls(urls.into_iter()));
            }

            if let Some(keywords) = poetry.keywords {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Keywords(keywords),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.display().to_string()),
                });
            }

            if let Some(authors) = poetry.authors {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Author(
                        authors.iter().map(|p| Person::from(p.as_str())).collect(),
                    ),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.display().to_string()),
                });
            }

            if let Some(homepage) = poetry.homepage {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Homepage(homepage),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.display().to_string()),
                });
            }

            if let Some(documentation) = poetry.documentation {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Documentation(documentation),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.display().to_string()),
                });
            }
        }
    }

    Ok(ret)
}

fn parse_python_project_urls(
    urls: impl Iterator<Item = (String, String)>,
) -> Vec<UpstreamDatumWithMetadata> {
    let mut ret = Vec::new();
    for (url_type, url) in urls {
        match url_type.as_str() {
            "GitHub" | "Repository" | "Source Code" | "Source" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(url.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: None,
                });
            }
            "Bug Tracker" | "Bug Reports" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::BugDatabase(url.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: None,
                });
            }
            "Documentation" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Documentation(url.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: None,
                });
            }
            "Funding" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Funding(url.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: None,
                });
            }
            u => {
                debug!("Unknown Python project URL type: {}", url_type);
            }
        }
    }
    ret
}

fn parse_python_long_description(
    long_description: &str,
    content_type: Option<&str>,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    if long_description.is_empty() {
        return Ok(vec![]);
    }
    let content_type = content_type.unwrap_or("text/plain");
    let mut content_type = content_type.split(';').next().unwrap();
    if long_description.contains("-*-restructuredtext-*-") {
        content_type = "text/restructured-text";
    }

    let mut ret = vec![];
    match content_type {
        "text/plain" => {
            let lines = long_description.split('\n').collect::<Vec<_>>();
            if lines.len() > 30 {
                debug!("Long description is too long ({} lines)", lines.len());
                return Ok(vec![]);
            }
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Description(long_description.to_string()),
                certainty: Some(Certainty::Possible),
                origin: None,
            });
        }
        "text/restructured-text" | "text/x-rst" => {
            let (description, extra_md) =
                crate::readme::description_from_readme_rst(long_description)
                    .map_err(|e| ProviderError::Other(e.to_string()))?;
            if let Some(description) = description {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Description(description),
                    certainty: Some(Certainty::Possible),
                    origin: Some("python long description (restructuredText)".to_string()),
                });
            }
            ret.extend(extra_md);
        }
        "text/markdown" => {
            let (description, extra_md) =
                crate::readme::description_from_readme_md(long_description)
                    .map_err(|e| ProviderError::Other(e.to_string()))?;
            if let Some(description) = description {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Description(description),
                    certainty: Some(Certainty::Possible),
                    origin: Some("python long description (markdown)".to_string()),
                });
            }
            ret.extend(extra_md);
        }
        _ => {
            warn!("Unknown content type: {}", content_type);
        }
    }
    Ok(ret)
}

pub fn parse_python_url(
    url: &str,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let repo = vcs::guess_repo_from_url(&url::Url::parse(url).unwrap(), None);
    if let Some(repo) = repo {
        return Ok(vec![UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository(repo),
            certainty: Some(Certainty::Likely),
            origin: None,
        }]);
    }

    Ok(vec![UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Homepage(url.to_string()),
        certainty: Some(Certainty::Likely),
        origin: None,
    }])
}

pub fn guess_from_setup_cfg(
    path: &Path,
    trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let setup_cfg =
        ini::Ini::load_from_file(path).map_err(|e| ProviderError::ParseError(e.to_string()))?;

    let metadata = match setup_cfg.section(Some("metadata")) {
        Some(metadata) => metadata,
        None => {
            debug!("No [metadata] section in setup.cfg");
            return Ok(vec![]);
        }
    };

    let mut ret = vec![];

    for (field, value) in metadata.iter() {
        match field {
            "name" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(value.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: None,
                });
            }
            "version" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(value.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: None,
                });
            }
            "url" => {
                ret.extend(parse_python_url(value)?);
            }
            "description" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(value.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: None,
                });
            }
            "long_description" => {
                if let Some(path) = value.strip_prefix(value) {
                    if path.contains('/') {
                        debug!("Ignoring long_description path: {}", path);
                        continue;
                    }
                    let value = match std::fs::read_to_string(path) {
                        Ok(value) => value,
                        Err(e) => {
                            debug!("Failed to read long_description file: {}", e);
                            continue;
                        }
                    };
                    ret.extend(parse_python_long_description(
                        &value,
                        metadata.get("long_description_content_type"),
                    )?);
                } else {
                    ret.extend(parse_python_long_description(
                        value,
                        metadata.get("long_description_content_type"),
                    )?);
                }
            }
            "maintainer" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Maintainer(Person {
                        name: Some(value.to_string()),
                        email: metadata.get("maintainer_email").map(|s| s.to_string()),
                        url: None,
                    }),
                    certainty: Some(Certainty::Certain),
                    origin: None,
                });
            }
            "author" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Author(vec![Person {
                        name: Some(value.to_string()),
                        email: metadata.get("author_email").map(|s| s.to_string()),
                        url: None,
                    }]),
                    certainty: Some(Certainty::Certain),
                    origin: None,
                });
            }
            "project_urls" => {
                let urls = value.split('\n').filter_map(|s| {
                    if s.is_empty() {
                        return None;
                    }
                    let (key, value) = match s.split_once('=') {
                        Some((key, value)) => (key, value),
                        None => {
                            debug!("Invalid project_urls line: {}", s);
                            return None;
                        }
                    };
                    Some((key.to_string(), value.to_string()))
                });
                ret.extend(parse_python_project_urls(urls));
            }
            "license" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License(value.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: None,
                });
            }
            "long_description_content_type" | "maintainer_email" | "author_email" => {
                // Ignore these, they are handled elsewhere
            }
            _ => {
                warn!("Unknown setup.cfg field: {}", field);
            }
        }
    }

    Ok(ret)
}
