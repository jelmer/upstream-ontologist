use crate::{py_to_upstream_datum, py_to_upstream_datum_with_metadata};
use crate::{vcs, Certainty, Person, UpstreamDatum, UpstreamDatumWithMetadata};
use log::{debug, warn};
use pyo3::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

pub fn guess_from_pkg_info(path: &Path, trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    let contents = std::fs::read(path).unwrap();
    let dist = python_pkginfo::Metadata::parse(contents.as_slice()).unwrap();

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
        ));
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

    ret
}

pub fn guess_from_pyproject_toml(
    path: &Path,
    trust_package: bool,
) -> Vec<UpstreamDatumWithMetadata> {
    let content = std::fs::read_to_string(path).unwrap();
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

    let pyproject: PyProjectToml = toml::from_str(content.as_str()).unwrap();

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

    ret
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
) -> Vec<UpstreamDatumWithMetadata> {
    if long_description.is_empty() {
        return vec![];
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
                return vec![];
            }
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Description(long_description.to_string()),
                certainty: Some(Certainty::Possible),
                origin: None,
            });
        }
        "text/restructured-text" | "text/x-rst" => Python::with_gil(|py| {
            let readme_mod = Python::import(py, "upstream_ontologist.readme").unwrap();
            let (description, extra_md): (Option<String>, Vec<PyObject>) = readme_mod
                .call_method1("description_from_readme_rst", (long_description,))
                .unwrap()
                .extract()
                .unwrap();

            if let Some(description) = description {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Description(description),
                    certainty: Some(Certainty::Possible),
                    origin: Some("python long description (restructuredText)".to_string()),
                });
            }
            ret.extend(
                extra_md
                    .into_iter()
                    .map(|m| py_to_upstream_datum_with_metadata(py, m))
                    .collect::<PyResult<Vec<_>>>()
                    .unwrap(),
            );
        }),
        "text/markdown" => Python::with_gil(|py| {
            let readme_mod = Python::import(py, "upstream_ontologist.readme").unwrap();
            let (description, extra_md): (Option<String>, Vec<PyObject>) = readme_mod
                .call_method1("description_from_readme_md", (long_description,))
                .unwrap()
                .extract()
                .unwrap();
            if let Some(description) = description {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Description(description),
                    certainty: Some(Certainty::Possible),
                    origin: Some("python long description (markdown)".to_string()),
                });
            }
            ret.extend(
                extra_md
                    .into_iter()
                    .map(|m| py_to_upstream_datum_with_metadata(py, m))
                    .collect::<PyResult<Vec<_>>>()
                    .unwrap(),
            );
        }),
        _ => {
            warn!("Unknown content type: {}", content_type);
        }
    }
    ret
}

pub fn parse_python_url(url: &str) -> Vec<UpstreamDatumWithMetadata> {
    let repo = vcs::guess_repo_from_url(&url::Url::parse(url).unwrap(), None);
    if let Some(repo) = repo {
        return vec![UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository(repo),
            certainty: Some(Certainty::Likely),
            origin: None,
        }];
    }

    vec![UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Homepage(url.to_string()),
        certainty: Some(Certainty::Likely),
        origin: None,
    }]
}