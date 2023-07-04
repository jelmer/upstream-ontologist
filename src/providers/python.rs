use crate::{py_to_upstream_datum, py_to_upstream_datum_with_metadata};
use crate::{vcs, Certainty, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use log::{debug, warn};
use pyo3::exceptions::PyAttributeError;
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
                ret.extend(parse_python_url(value));
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

fn guess_from_setup_py_executed(
    path: &Path,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut ret = Vec::new();
    // Import setuptools, just in case it replaces distutils
    use pyo3::types::PyDict;
    let mut long_description = None;
    Python::with_gil(|py| {
        let _ = py.import("setuptools");

        let run_setup = py.import("distutils.core")?.getattr("run_setup")?;

        let os = py.import("os")?;

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
            let kwargs = PyDict::new(py);
            kwargs.set_item("stop_after", "config")?;

            run_setup.call((path,), Some(kwargs))
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

        if let Some(url) = result.call_method0("get_url")?.extract()? {
            ret.extend(parse_python_url(url));
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
                origin: None,
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
                origin: None,
            });
        }

        if let Some(description) = result.call_method0("get_description")?.extract()? {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Summary(description),
                certainty: Some(Certainty::Certain),
                origin: None,
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

/*
#[cfg(feature = "python-parser")]
fn guess_from_setup_py_parsed(
    path: &Path,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let code = match std::fs::read_to_string(path) {
        Ok(setup_text) => setup_text,
        Err(e) => {
            warn!("Failed to read setup.py: {}", e);
            return Err(ProviderError::Other(e.to_string()));
        }
    };

    use python_parser::ast;
    let ast = match python_parser::file_input(python_parser::make_strspan(code.as_str())) {
        Ok(ast) => ast,
        Err(e) => {
            warn!("Failed to parse setup.py: {}", e);
            return Err(ProviderError::Other(e.to_string()));
        }
    }
    .1;

    let setup_args = HashMap::new();

    for statement in ast {
        // We only care about function calls or assignments to functions named
        // `setup` or `main`
        match statement {
            ast::Statement:Expression(expr) => {
                if let ast::Expression::Call(call, args) = expr {
                    if let ast::Expression::Name(name) = call {
                        if name.to_string() == "setup" || name.to_string() == "main" {
                            // Process the arguments to the setup function
                            for arg in args {
                                match arg {
                                    ast::Argument::Keyword(name, value) => {
                                        setup_args.insert(name, value);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn get_str_from_expr(expr: &ast::Expression) -> Option<String> {
        if let ast::Expression::String(value) = expr {
            Some(value.into_iter().map(|x| x.content.into_string().unwrap() ).collect::<Vec<String>>().concat())
        } else {
            None
        }
    }

    fn get_list_from_expr(expr: &ast::Expression) -> Option<Vec<String>> {
        if let ast::Expression::ListLiteral(list) = expr {
            // We collect the elements of a list if the element
            // and tag function calls
            Some(
                list.iter()
                    .filter_map(|elt| get_str_from_expr(elt.))
                    .collect(),
            )
        } else {
            None
        }
    }

    let mut ret = Vec::new();

    if let Some(name) = setup_args.get("name") {
        if let Some(value) = get_str_from_expr(name) {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(value),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        } else {
            debug!("Unable to parse {:?} as a string", name);
        }
    }

    if let Some(version) = setup_args.get("version") {
        if let Some(value) = get_str_from_expr(version) {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Version(value),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        } else {
            debug!("Unable to parse {:?} as a string", version);
        }
    }

    if let Some(description) = setup_args.get("description") {
        if let Some(value) = get_str_from_expr(description) {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Summary(value),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        } else {
            debug!("Unable to parse {:?} as a string", description);
        }
    }

    if let Some(long_description) = setup_args.get("long_description") {
        if let Some(value) = get_str_from_expr(long_description) {
            let content_type =
                setup_args
                    .get("long_description_content_type")
                    .map(|x| get_str_from_expr(x)).flatten();
            ret.extend(parse_python_long_description(
                value.as_str(),
                content_type.as_deref(),
            )?);
        } else {
            debug!("Unable to parse {:?} as a string", long_description);
        }
    }

    if let Some(license) = setup_args.get("license") {
        if let Some(value) = get_str_from_expr(license) {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::License(value),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        } else {
            debug!("Unable to parse {:?} as a string", license);
        }
    }

    if let Some(download_url) = setup_args.get("download_url") {
        if let Some(value) = get_str_from_expr(download_url) {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Download(value),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        } else {
            debug!("Unable to parse {:?} as a string", download_url);
        }
    }

    if let Some(url) = setup_args.get("url") {
        if let Some(value) = get_str_from_expr(url) {
            ret.extend(parse_python_url(value.as_str())?);
        } else {
            debug!("Unable to parse {:?} as a string", url);
        }
    }

    if let Some(project_urls) = setup_args.get("project_urls") {
        if let Some(value) = get_list_from_expr(project_urls) {
            ret.extend(parse_python_project_urls(value.into_iter()));
        } else {
            debug!("Unable to parse {:?} as a list", project_urls);
        }
    }

    if let Some(maintainer) = setup_args.get("maintainer") {
        if let Some(value) = get_list_from_expr(maintainer) {
            if value.len() >= 1 {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Maintainer(Person::from(value[0])),
                    certainty: Some(Certainty::Certain),
                    origin: None,
                });
            }
        } else if let Some(value) = get_str_from_expr(maintainer) {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Maintainer(Person {
                    name: Some(value),
                    email: setup_args
                        .get("maintainer_email")
                        .map(|x| get_str_from_expr(x)).flatten(),
                    url: None,
                }),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        } else {
            debug!("Unable to parse {:?} as a string", maintainer);
        }
    }

    if let Some(author) = setup_args.get("author") {
        if let value = get_list_from_expr(author) {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Author(value.iter().map(|x| Person::from(x)).collect()),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        } else if let Some(value) = get_str_from_expr(author) {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Author(vec![Person {
                    name: Some(value),
                    email: setup_args.get("author_email").map(|x| get_str_from_expr(x)).flatten(),
                    url: None,
                }]),
                certainty: Some(Certainty::Certain),
                origin: None,
            });
        } else {
            debug!("Unable to parse {:?} as a string", author);
        }
    }

    Ok(ret)
}
*/

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
        let ast = py.import("ast").unwrap();

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
            if (statement.is_instance(ast_expr)?
                || statement.is_instance(ast_call)?
                || statement.is_instance(ast_assign)?)
                && statement.getattr("value")?.is_instance(ast_call)?
                && statement
                    .getattr("value")?
                    .getattr("func")?
                    .is_instance(ast_name)?
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

        let get_str_from_expr = |expr: &PyAny| -> Option<String> {
            if expr.is_instance(ast_str).ok()? {
                Some(expr.getattr("s").ok()?.extract::<String>().ok()?)
            } else if expr.is_instance(ast_constant).ok()? {
                Some(expr.getattr("value").ok()?.extract::<String>().ok()?)
            } else {
                None
            }
        };

        let ast_list = ast.getattr("List").unwrap();
        let ast_tuple = ast.getattr("Tuple").unwrap();
        let ast_set = ast.getattr("Set").unwrap();

        let get_str_list_from_expr = |expr: &PyAny| -> Option<Vec<String>> {
            // We collect the elements of a list if the element
            // and tag function calls
            if expr.is_instance(ast_list).ok()?
                || expr.is_instance(ast_tuple).ok()?
                || expr.is_instance(ast_set).ok()?
            {
                let mut ret = Vec::new();
                for elt in expr.getattr("elts").ok()?.iter().ok()? {
                    let elt = elt.ok()?;
                    if let Some(value) = get_str_from_expr(elt) {
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

        let ast_dict = py.import("ast.Dict").unwrap();

        let get_dict_from_expr = |expr: &PyAny| -> Option<HashMap<String, String>> {
            if expr.is_instance(ast_dict).ok()? {
                let mut ret = HashMap::new();
                let keys = expr.getattr("keys").ok()?;
                let values = expr.getattr("values").ok()?;
                for (key, value) in keys.iter().ok()?.zip(values.iter().ok()?) {
                    if let Some(key) = get_str_from_expr(key.ok()?) {
                        if let Some(value) = get_str_from_expr(value.ok()?) {
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
            let value = value.as_ref(py);
            match key.as_str() {
                "name" => {
                    if let Some(name) = get_str_from_expr(value) {
                        ret.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Name(name),
                            certainty: Some(Certainty::Certain),
                            origin: Some("setup.py".to_string()),
                        });
                    }
                }
                "version" => {
                    if let Some(version) = get_str_from_expr(value) {
                        ret.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Version(version),
                            certainty: Some(Certainty::Certain),
                            origin: Some("setup.py".to_string()),
                        });
                    }
                }
                "description" => {
                    if let Some(description) = get_str_from_expr(value) {
                        ret.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Summary(description),
                            certainty: Some(Certainty::Certain),
                            origin: Some("setup.py".to_string()),
                        });
                    }
                }
                "long_description" => {
                    if let Some(description) = get_str_from_expr(value) {
                        let content_type = setup_args.get("long_description_content_type");
                        let content_type = if let Some(content_type) = content_type {
                            get_str_from_expr(content_type.as_ref(py))
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
                            origin: Some("setup.py".to_string()),
                        });
                    }
                }
                "download_url" => {
                    if let Some(download_url) = get_str_from_expr(value) {
                        ret.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Download(download_url),
                            certainty: Some(Certainty::Certain),
                            origin: Some("setup.py".to_string()),
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
                        ret.extend(parse_python_project_urls(project_urls.into_iter()));
                    }
                }
                "maintainer" => {
                    if let Some(maintainer) = get_str_from_expr(value) {
                        let maintainer_email = setup_args.get("maintainer_email");
                        let maintainer_email = if let Some(maintainer_email) = maintainer_email {
                            get_str_from_expr(maintainer_email.as_ref(py))
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
                            origin: Some("setup.py".to_string()),
                        });
                    }
                }
                "author" => {
                    if let Some(author) = get_str_from_expr(value) {
                        let author_email = setup_args.get("author_email");
                        let author_email = if let Some(author_email) = author_email {
                            get_str_from_expr(author_email.as_ref(py))
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
                            origin: Some("setup.py".to_string()),
                        });
                    } else if let Some(author) = get_str_list_from_expr(value) {
                        let author_emails = setup_args.get("author_email");
                        let author_emails = if let Some(author_emails) = author_emails {
                            get_str_list_from_expr(author_emails.as_ref(py)).map_or_else(|| vec![None; author.len()], |v| v.into_iter().map(Some).collect())
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
                            origin: Some("setup.py".to_string()),
                        });
                    }
                }
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
        )?);
    }

    Ok(ret)
}
