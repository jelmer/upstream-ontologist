use serde::Deserialize;
use crate::{
    vcs, Certainty, GuesserSettings, Origin, Person, ProviderError, UpstreamDatum,
    UpstreamDatumWithMetadata,, UpstreamMetadata
};
use log::{debug, warn};

use pyo3::prelude::*;
use std::collections::HashMap;
use std::path::Path;

pub fn guess_from_pkg_info(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let contents = std::fs::read(path)?;
    let dist = python_pkginfo::Metadata::parse(contents.as_slice()).map_err(|e| {
        ProviderError::ParseError(format!("Failed to parse python package metadata: {}", e))
    })?;

    let mut ret = vec![];

    ret.push(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name(dist.name),
        certainty: Some(Certainty::Certain),
        origin: Some(path.into()),
    });

    ret.push(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Version(dist.version),
        certainty: Some(Certainty::Certain),
        origin: Some(path.into()),
    });

    if let Some(homepage) = dist.home_page {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Homepage(homepage),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(summary) = dist.summary {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Summary(summary),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(description) = dist.description {
        ret.extend(parse_python_long_description(
            description.as_str(),
            dist.description_content_type.as_deref(),
            &Origin::Path(path.to_path_buf()),
        )?);
    }

    ret.extend(parse_python_project_urls(
        dist.project_urls
            .iter()
            .map(|k| k.split_once(", ").unwrap())
            .map(|(k, v)| (k.to_string(), v.to_string())),
        &Origin::Path(path.to_path_buf()),
    ));

    if dist.author.is_some() || dist.author_email.is_some() {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Author(vec![Person {
                name: dist.author,
                email: dist.author_email,
                url: None,
            }]),

            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
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
            origin: Some(path.into()),
        });
    }

    if let Some(license) = dist.license {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::License(license),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(keywords) = dist.keywords {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Keywords(keywords.split(", ").map(|s| s.to_string()).collect()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(download_url) = dist.download_url {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Download(download_url),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    Ok(ret)
}

pub fn guess_from_pyproject_toml(
    path: &Path,
    _settings: &GuesserSettings,
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

    let pyproject: PyProjectToml =
        toml::from_str(content.as_str()).map_err(|e| ProviderError::ParseError(e.to_string()))?;

    if let Some(inner_project) = pyproject.inner.project {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(inner_project.name),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });

        if let Some(version) = inner_project.version {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Version(version.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }

        if let Some(pyproject_toml::License::String(license)) = inner_project.license.as_ref() {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::License(license.clone()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
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
                origin: Some(path.into()),
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
                origin: Some(path.into()),
            });
        }

        if let Some(keywords) = inner_project.keywords {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Keywords(keywords),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }

        if let Some(urls) = inner_project.urls {
            ret.extend(parse_python_project_urls(
                urls.into_iter(),
                &Origin::Path(path.to_path_buf()),
            ));
        }

        if let Some(classifiers) = inner_project.classifiers {
            ret.extend(parse_python_classifiers(
                classifiers.iter().map(|s| s.as_str()),
                &Origin::Path(path.to_path_buf()),
            ));
        }
    }

    if let Some(tool) = pyproject.tool {
        if let Some(poetry) = tool.poetry {
            if let Some(version) = poetry.version {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(version),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }

            if let Some(description) = poetry.description {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(description),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }

            if let Some(license) = poetry.license {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License(license),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }

            if let Some(repository) = poetry.repository {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(repository),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }

            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(poetry.name.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });

            if let Some(urls) = poetry.urls {
                ret.extend(parse_python_project_urls(
                    urls.into_iter(),
                    &Origin::Path(path.to_path_buf()),
                ));
            }

            if let Some(keywords) = poetry.keywords {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Keywords(keywords),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }

            if let Some(authors) = poetry.authors {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Author(
                        authors.iter().map(|p| Person::from(p.as_str())).collect(),
                    ),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }

            if let Some(homepage) = poetry.homepage {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Homepage(homepage),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }

            if let Some(documentation) = poetry.documentation {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Documentation(documentation),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
        }
    }

    Ok(ret)
}

fn parse_python_project_urls(
    urls: impl Iterator<Item = (String, String)>,
    origin: &Origin,
) -> Vec<UpstreamDatumWithMetadata> {
    let mut ret = Vec::new();
    for (url_type, url) in urls {
        match url_type.as_str() {
            "GitHub" | "Repository" | "Source Code" | "Source" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(url.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(origin.clone()),
                });
            }
            "Bug Tracker" | "Bug Reports" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::BugDatabase(url.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(origin.clone()),
                });
            }
            "Documentation" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Documentation(url.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(origin.clone()),
                });
            }
            "Funding" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Funding(url.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(origin.clone()),
                });
            }
            "Homepage" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Homepage(url.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(origin.clone()),
                });
            }
            _u => {
                debug!("Unknown Python project URL type: {}", url_type);
            }
        }
    }
    ret
}

fn parse_python_long_description(
    long_description: &str,
    content_type: Option<&str>,
    origin: &Origin,
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
                origin: Some(origin.clone()),
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
                    origin: Some(Origin::Other(
                        "python long description (restructuredText)".to_string(),
                    )),
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
                    origin: Some(Origin::Other(
                        "python long description (markdown)".to_string(),
                    )),
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

pub fn guess_from_setup_cfg(
    path: &Path,
    _settings: &GuesserSettings,
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

    let origin = Origin::Path(path.to_path_buf());

    let mut ret = vec![];

    for (field, value) in metadata.iter() {
        match field {
            "name" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(value.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(origin.clone()),
                });
            }
            "version" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(value.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(origin.clone()),
                });
            }
            "url" => {
                ret.extend(parse_python_url(value));
            }
            "description" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(value.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(origin.clone()),
                });
            }
            "summary" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(value.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(origin.clone()),
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
                        &origin,
                    )?);
                } else {
                    ret.extend(parse_python_long_description(
                        value,
                        metadata.get("long_description_content_type"),
                        &origin,
                    )?);
                }
            }
            "maintainer" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Maintainer(Person {
                        name: Some(value.to_string()),
                        email: metadata
                            .get("maintainer_email")
                            .or_else(|| metadata.get("maintainer-email"))
                            .map(|s| s.to_string()),
                        url: None,
                    }),
                    certainty: Some(Certainty::Certain),
                    origin: Some(origin.clone()),
                });
            }
            "author" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Author(vec![Person {
                        name: Some(value.to_string()),
                        email: metadata
                            .get("author_email")
                            .or_else(|| metadata.get("author-email"))
                            .map(|s| s.to_string()),
                        url: None,
                    }]),
                    certainty: Some(Certainty::Certain),
                    origin: Some(origin.clone()),
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
                ret.extend(parse_python_project_urls(urls, &origin));
            }
            "license" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License(value.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(origin.clone()),
                });
            }
            "home-page" => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Homepage(value.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(origin.clone()),
                });
            }
            "long_description_content_type"
            | "maintainer_email"
            | "author_email"
            | "maintainer-email"
            | "author-email" => {
                // Ignore these, they are handled elsewhere
            }
            _ => {
                warn!("Unknown setup.cfg field: {}", field);
            }
        }
    }

    Ok(ret)
}

fn guess_from_setup_py_executed(
    path: &Path,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    // Ensure only one thread can run this function at a time
    static SETUP_PY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _lock = SETUP_PY_LOCK.lock().unwrap();
    let mut ret = Vec::new();
    // Import setuptools, just in case it replaces distutils
    use pyo3::types::PyDict;
    let mut long_description = None;
    Python::with_gil(|py| {
        let _ = py.import_bound("setuptools");

        let run_setup = py.import_bound("distutils.core")?.getattr("run_setup")?;

        let os = py.import_bound("os")?;

        let orig = match os.getattr("getcwd")?.call0() {
            Ok(orig) => Some(orig.extract::<String>()?),
            Err(e) => {
                debug!("Failed to get current directory: {}", e);
                None
            }
        };

        let parent = path.parent().unwrap();

        os.getattr("chdir")?.call1((parent,))?;

        let result = || -> PyResult<_> {
            let kwargs = PyDict::new_bound(py);
            kwargs.set_item("stop_after", "config")?;

            run_setup.call((path,), Some(&kwargs))
        }();

        if let Some(orig) = orig {
            os.getattr("chdir")?.call1((orig,))?;
        }

        let result = result?;

        if let Some(name) = result.call_method0("get_name")?.extract()? {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(name),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        if let Some(version) = result.call_method0("get_version")?.extract()? {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Version(version),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        if let Some(url) = result
            .call_method0("get_url")?
            .extract::<Option<String>>()?
        {
            ret.extend(parse_python_url(&url));
        }

        if let Some(download_url) = result.call_method0("get_download_url")?.extract()? {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Download(download_url),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        if let Some(license) = result.call_method0("get_license")?.extract()? {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::License(license),
                certainty: Some(Certainty::Likely),
                origin: Some(Origin::Path(path.to_path_buf())),
            });
        }

        if let Some(contact) = result.call_method0("get_contact")?.extract()? {
            let contact: String = match result
                .call_method0("get_contact_email")?
                .extract::<Option<String>>()?
            {
                Some(email) => format!("{} <{}>", contact, email),
                None => contact,
            };
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Contact(contact),
                certainty: Some(Certainty::Certain),
                origin: Some(Origin::Path(path.to_path_buf())),
            });
        }

        if let Some(description) = result.call_method0("get_description")?.extract()? {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Summary(description),
                certainty: Some(Certainty::Certain),
                origin: Some(Origin::Path(path.to_path_buf())),
            });
        }

        if let Some(description) = result
            .call_method0("get_long_description")?
            .extract::<Option<String>>()?
        {
            let content_type = match result.getattr("long_description_content_type") {
                Ok(content_type) => content_type.extract::<Option<String>>(),
                Err(e) if e.is_instance_of::<pyo3::exceptions::PyAttributeError>(py) => Ok(None),
                Err(e) => return Err(e),
            }?;
            long_description = Some((description, content_type));
        }

        if let Ok(metadata) = result.getattr("metadata") {
            if let Ok(project_urls) = metadata.getattr("project_urls") {
                ret.extend(parse_python_project_urls(
                    project_urls
                        .extract::<HashMap<String, String>>()?
                        .into_iter(),
                    &Origin::Path(path.to_path_buf()),
                ));
            }
        }
        Ok::<(), PyErr>(())
    })
    .map_err(|e| {
        warn!("Failed to run setup.py: {}", e);
        ProviderError::Other(e.to_string())
    })?;

    if let Some((long_description, long_description_content_type)) = long_description {
        ret.extend(parse_python_long_description(
            long_description.as_str(),
            long_description_content_type.as_deref(),
            &Origin::Path(path.to_path_buf()),
        )?);
    }

    Ok(ret)
}

pub fn guess_from_setup_py(
    path: &Path,
    trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    if trust_package {
        guess_from_setup_py_executed(path)
    } else {
        guess_from_setup_py_parsed(path)
    }
}

fn guess_from_setup_py_parsed(
    path: &Path,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let code = match std::fs::read_to_string(path) {
        Ok(setup_text) => setup_text,
        Err(e) => {
            warn!("Failed to read setup.py: {}", e);
            return Err(ProviderError::IoError(e));
        }
    };

    let mut long_description = None;
    let mut ret = Vec::new();

    Python::with_gil(|py| {
        let ast = py.import_bound("ast").unwrap();

        // Based on pypi.py in https://github.com/nexB/scancode-toolkit/blob/develop/src/packagedcode/pypi.py
        //
        // Copyright (c) nexB Inc. and others. All rights reserved.
        // ScanCode is a trademark of nexB Inc.
        // SPDX-License-Identifier: Apache-2.0

        let tree = ast.call_method1("parse", (code,))?;
        let mut setup_args: HashMap<String, PyObject> = HashMap::new();

        let ast_expr = ast.getattr("Expr").unwrap();
        let ast_call = ast.getattr("Call").unwrap();
        let ast_assign = ast.getattr("Assign").unwrap();
        let ast_name = ast.getattr("Name").unwrap();

        for statement in tree.getattr("body")?.iter()? {
            let statement = statement?;
            // We only care about function calls or assignments to functions named
            // `setup` or `main`
            if (statement.is_instance(&ast_expr)?
                || statement.is_instance(&ast_call)?
                || statement.is_instance(&ast_assign)?)
                && statement.getattr("value")?.is_instance(&ast_call)?
                && statement
                    .getattr("value")?
                    .getattr("func")?
                    .is_instance(&ast_name)?
                && (statement.getattr("value")?.getattr("func")?.getattr("id")?.extract::<String>()? == "setup" ||
                    // we also look for main as sometimes this is used instead of
                    // setup()
                    statement.getattr("value")?.getattr("func")?.getattr("id")?.extract::<String>()? == "main")
            {
                let value = statement.getattr("value")?;

                // Process the arguments to the setup function
                for kw in value.getattr("keywords")?.iter()? {
                    let kw = kw?;
                    let arg_name = kw.getattr("arg")?.extract::<String>()?;

                    setup_args.insert(arg_name, kw.getattr("value")?.to_object(py));
                }
            }
        }

        // End code from https://github.com/nexB/scancode-toolkit/blob/develop/src/packagedcode/pypi.py

        let ast_str = ast.getattr("Str").unwrap();
        let ast_constant = ast.getattr("Constant").unwrap();

        let get_str_from_expr = |expr: &Bound<PyAny>| -> Option<String> {
            if expr.is_instance(&ast_str).ok()? {
                Some(expr.getattr("s").ok()?.extract::<String>().ok()?)
            } else if expr.is_instance(&ast_constant).ok()? {
                Some(expr.getattr("value").ok()?.extract::<String>().ok()?)
            } else {
                None
            }
        };

        let ast_list = ast.getattr("List").unwrap();
        let ast_tuple = ast.getattr("Tuple").unwrap();
        let ast_set = ast.getattr("Set").unwrap();

        let get_str_list_from_expr = |expr: &Bound<PyAny>| -> Option<Vec<String>> {
            // We collect the elements of a list if the element
            // and tag function calls
            if expr.is_instance(&ast_list).ok()?
                || expr.is_instance(&ast_tuple).ok()?
                || expr.is_instance(&ast_set).ok()?
            {
                let mut ret = Vec::new();
                for elt in expr.getattr("elts").ok()?.iter().ok()? {
                    let elt = elt.ok()?;
                    if let Some(value) = get_str_from_expr(&elt) {
                        ret.push(value);
                    } else {
                        return None;
                    }
                }
                Some(ret)
            } else {
                None
            }
        };

        let ast = py.import_bound("ast").unwrap();
        let ast_dict = ast.getattr("Dict").unwrap();

        let get_dict_from_expr = |expr: &Bound<PyAny>| -> Option<HashMap<String, String>> {
            if expr.is_instance(&ast_dict).ok()? {
                let mut ret = HashMap::new();
                let keys = expr.getattr("keys").ok()?;
                let values = expr.getattr("values").ok()?;
                for (key, value) in keys.iter().ok()?.zip(values.iter().ok()?) {
                    if let Some(key) = get_str_from_expr(&key.ok()?) {
                        if let Some(value) = get_str_from_expr(&value.ok()?) {
                            ret.insert(key, value);
                        } else {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
                Some(ret)
            } else {
                None
            }
        };

        // TODO: what if kw.value is an expression like a call to
        // version=get_version or version__version__

        for (key, value) in setup_args.iter() {
            let value = value.bind(py);
            match key.as_str() {
                "name" => {
                    if let Some(name) = get_str_from_expr(value) {
                        ret.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Name(name),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.into())
                        });
                    }
                }
                "version" => {
                    if let Some(version) = get_str_from_expr(value) {
                        ret.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Version(version),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.into())
                        });
                    }
                }
                "description" => {
                    if let Some(description) = get_str_from_expr(value) {
                        ret.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Summary(description),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.into())
                        });
                    }
                }
                "long_description" => {
                    if let Some(description) = get_str_from_expr(value) {
                        let content_type = setup_args.get("long_description_content_type");
                        let content_type = if let Some(content_type) = content_type {
                            get_str_from_expr(content_type.bind(py))
                        } else {
                            None
                        };
                        long_description = Some((description, content_type));
                    }
                }
                "license" => {
                    if let Some(license) = get_str_from_expr(value) {
                        ret.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::License(license),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.into()),
                        });
                    }
                }
                "download_url" => {
                    if let Some(download_url) = get_str_from_expr(value) {
                        ret.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Download(download_url),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.into()),
                        });
                    }
                }
                "url" => {
                    if let Some(url) = get_str_from_expr(value) {
                        ret.extend(parse_python_url(url.as_str()));
                    }
                }
                "project_urls" => {
                    if let Some(project_urls) = get_dict_from_expr(value) {
                        ret.extend(parse_python_project_urls(project_urls.into_iter(), &Origin::Path(path.into())));
                    }
                }
                "maintainer" => {
                    if let Some(maintainer) = get_str_from_expr(value) {
                        let maintainer_email = setup_args.get("maintainer_email");
                        let maintainer_email = if let Some(maintainer_email) = maintainer_email {
                            get_str_from_expr(maintainer_email.bind(py))
                        } else {
                            None
                        };
                        ret.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Maintainer(Person {
                                name: Some(maintainer),
                                email: maintainer_email,
                                url: None
                            }),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.into()),
                        });
                    }
                }
                "author" => {
                    if let Some(author) = get_str_from_expr(value) {
                        let author_email = setup_args.get("author_email");
                        let author_email = if let Some(author_email) = author_email {
                            get_str_from_expr(author_email.bind(py))
                        } else {
                            None
                        };
                        ret.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Author(vec![Person {
                                name: Some(author),
                                email: author_email,
                                url: None
                            }]),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.into()),
                        });
                    } else if let Some(author) = get_str_list_from_expr(value) {
                        let author_emails = setup_args.get("author_email");
                        let author_emails = if let Some(author_emails) = author_emails {
                            get_str_list_from_expr(author_emails.bind(py)).map_or_else(|| vec![None; author.len()], |v| v.into_iter().map(Some).collect())
                        } else {
                            vec![None; author.len()]
                        };
                        let persons = author.into_iter().zip(author_emails.into_iter()).map(|(name, email)| Person {
                            name: Some(name),
                            email,
                            url: None
                        }).collect();
                        ret.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Author(persons),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.into()),
                        });
                    }
                }
                "keywords" => {
                    if let Some(keywords) = get_str_list_from_expr(value) {
                        ret.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Keywords(keywords),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.into()),
                        });
                    }
                }
                "classifiers" => {
                    if let Some(classifiers) = get_str_list_from_expr(value) {
                        ret.extend(parse_python_classifiers(classifiers.iter().map(|s| s.as_str()), &Origin::Path(path.into())));
                    }
                }
                // Handled above
                "author_email" | "maintainer_email" => {},
                // Irrelevant
                "rust_extensions" | "data_files" | "packages" | "package_dir" | "entry_points" => {},
                // Irrelevant: dependencies
                t if t.ends_with("_requires") || t.ends_with("_require") => {},
                _ => {
                    warn!("Unknown key in setup.py: {}", key);
                }
            }
        }
        Ok::<(), PyErr>(())
    }).map_err(|e: PyErr| {
        Python::with_gil(|py| {
        if e.is_instance_of::<pyo3::exceptions::PySyntaxError>(py) {
                warn!("Syntax error while parsing setup.py: {}", e);
                ProviderError::Other(e.to_string())
            } else {
                warn!("Failed to parse setup.py: {}", e);
                ProviderError::Other(e.to_string())
        }
        })
    })?;

    if let Some((description, content_type)) = long_description {
        ret.extend(parse_python_long_description(
            description.as_str(),
            content_type.as_deref(),
            &Origin::Path(path.into()),
        )?);
    }

    Ok(ret)
}

fn parse_python_classifiers<'a>(
    classifiers: impl Iterator<Item = &'a str> + 'a,
    origin: &'a Origin,
) -> impl Iterator<Item = UpstreamDatumWithMetadata> + 'a {
    classifiers.filter_map(|classifier| {
        let mut parts = classifier.split(" :: ");
        let category = parts.next()?;
        let subcategory = parts.next()?;
        let value = parts.next()?;
        let certainty = Some(Certainty::Certain);
        let origin = Some(origin.clone());
        match (category, subcategory) {
            ("Development Status", _) => None,
            ("Intended Audience", _) => None,
            ("License", "OSI Approved") => Some(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::License(value.into()),
                certainty,
                origin,
            }),
            ("Natural Language", _) => None,
            ("Operating System", _) => None,
            ("Programming Language", _) => None,
            ("Topic", _) => None,
            _ => {
                warn!("Unknown classifier: {}", classifier);
                None
            }
        }
    })
}

#[derive(Deserialize)]
pub struct PypiProjectInfo {
    pub author: Option<String>,
    pub author_email: Option<String>,
    pub bugtrack_url: Option<String>,
    pub classifiers: Vec<String>,
    pub description: String,
    pub description_content_type: Option<String>,
    pub docs_url: Option<String>,
    pub download_url: Option<String>,
    pub downloads: HashMap<String, isize>,
    pub dynamic: Option<bool>,
    pub home_page: Option<String>,
    pub keywords: Option<String>,
    pub license: Option<String>,
    pub maintainer: Option<String>,
    pub maintainer_email: Option<String>,
    pub name: String,
    pub package_url: String,
    pub platform: Option<String>,
    pub project_url: String,
    pub project_urls: Option<HashMap<String, String>>,
    pub provides_extra: Option<bool>,
    pub release_url: String,
    pub requires_dist: Option<Vec<String>>,
    pub requires_python: Option<String>,
    pub summary: String,
    pub version: String,
    pub yanked: Option<bool>,
    pub yanked_reason: Option<String>,
}

#[derive(Deserialize)]
pub struct Digests {
    pub md5: String,
    pub sha256: String,
    pub blake2b_256: String,
}

#[derive(Deserialize)]
pub struct PypiRelease {
    pub comment_text: String,
    pub digests: Digests,
    pub downloads: isize,
    pub filename: String,
    pub has_sig: bool,
    pub md5_digest: String,
    pub packagetype: String,
    pub python_version: String,
    pub requires_python: Option<String>,
    pub size: isize,
    pub upload_time: String,
    pub upload_time_iso_8601: String,
    pub url: String,
    pub yanked: bool,
    pub yanked_reason: Option<String>,
}

#[derive(Deserialize)]
pub struct PypiUrl {
    pub comment_text: String,
    pub digests: Digests,
    pub filename: String,
    pub has_sig: bool,
    pub packagetype: String,
    pub python_version: String,
    pub requires_python: Option<String>,
    pub size: isize,
    pub upload_time: String,
    pub upload_time_iso_8601: String,
    pub url: String,
    pub yanked: bool,
    pub yanked_reason: Option<String>,
}

#[derive(Deserialize)]
pub struct PypiProject {
    pub info: PypiProjectInfo,
    pub last_serial: isize,
    pub releases: HashMap<String, Vec<PypiRelease>>,
    pub urls: Vec<PypiUrl>,
    pub vulnerabilities: Vec<String>,

}

impl TryInto<UpstreamMetadata> for PypiProject {
    type Error = ProviderError;

    fn try_into(self) -> Result<UpstreamMetadata, Self::Error> {
        let mut metadata = UpstreamMetadata::default();
        if let Some(author) = self.info.author {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Author(vec![Person {
                    name: Some(author),
                    email: self.info.author_email,
                    url: None,
                }]),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Description(self.info.description),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        if let Some(homepage) = self.info.home_page {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Homepage(homepage),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        if let Some(license) = self.info.license {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::License(license),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(self.info.name),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        if let Some(maintainer) = self.info.maintainer {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Maintainer(Person {
                    name: Some(maintainer),
                    email: self.info.maintainer_email,
                    url: None,
                }),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Version(self.info.version),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        if let Some(keywords) = self.info.keywords {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Keywords(keywords.split(',').map(|s| s.trim().to_string()).collect()),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        if let Some(urls) = self.info.project_urls {
            metadata.0.extend(parse_python_project_urls(
                urls.into_iter(),
                &Origin::Other("pypi".to_string()),
            ));
        }

        if let Some(description) = self.info.summary {
            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Summary(description),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        }

        Ok(metadata)
    }
}

pub fn load_pypi_project(name: &str) -> Result<Option<PypiProject>, ProviderError> {
    let http_url = format!("https://pypi.org/pypi/{}/json", name).parse().unwrap();
    let data = crate::load_json_url(&http_url, None)?;
    let pypi_data: PypiProject = serde_json::from_value(data).map_err(|e| crate::ProviderError::Other(e.to_string()))?;
    Ok(Some(pypi_data))
}

pub fn remote_pypi_metadata(name: &str) -> Result<UpstreamMetadata, ProviderError> {
    let pypi = load_pypi_project(name)?;

    match pypi {
        Some(pypi) => pypi.try_into(),
        None => Ok(UpstreamMetadata::default()),
    }
}

#[cfg(test)]
mod pypi_tests {
    use super::*;

    #[test]
    fn test_pypi_upstream_info() {
        let data = include_str!("../testdata/pypi.json");

        let pypi_data: PypiProject = serde_json::from_str(data).unwrap();

        assert_eq!(pypi_data.info.name, "merge3");
    }
}
