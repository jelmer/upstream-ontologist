use log::{error, warn};
use pyo3::prelude::*;
use regex::Regex;
use reqwest::header::HeaderMap;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use url::Url;
use xmltree::{Element, XMLNode};

lazy_static::lazy_static! {
    static ref USER_AGENT: String = String::from("upstream-ontologist/") + env!("CARGO_PKG_VERSION");
}
// Too aggressive?
const DEFAULT_URLLIB_TIMEOUT: u64 = 3;

pub mod vcs;

#[derive(Debug, Ord, Eq, PartialOrd, PartialEq)]
pub enum Certainty {
    Certain,
    Confident,
    Likely,
    Possible,
}

impl From<&str> for Certainty {
    fn from(s: &str) -> Self {
        match s {
            "certain" => Certainty::Certain,
            "confident" => Certainty::Confident,
            "likely" => Certainty::Likely,
            "possible" => Certainty::Possible,
            _ => panic!("unknown certainty: {}", s),
        }
    }
}

impl ToString for Certainty {
    fn to_string(&self) -> String {
        match self {
            Certainty::Certain => "certain".to_string(),
            Certainty::Confident => "confident".to_string(),
            Certainty::Likely => "likely".to_string(),
            Certainty::Possible => "possible".to_string(),
        }
    }
}

pub struct Person {
    pub name: Option<String>,
    pub email: Option<String>,
    pub url: Option<String>,
}

impl Default for Person {
    fn default() -> Self {
        Person {
            name: None,
            email: None,
            url: None,
        }
    }
}

impl From<&str> for Person {
    fn from(text: &str) -> Self {
        let mut text = text.replace(" at ", "@");
        text = text.replace(" -at- ", "@");
        text = text.replace(" -dot- ", ".");
        text = text.replace("[AT]", "@");

        if text.contains('(') && text.ends_with(')') {
            if let Some((p1, p2)) = text[..text.len() - 1].split_once('(') {
                if p2.starts_with("https://") || p2.starts_with("http://") {
                    let url = p2.to_string();
                    if let Some((name, email)) = parseaddr(p1) {
                        return Person {
                            name: Some(name),
                            email: Some(email),
                            url: Some(url),
                        };
                    } else {
                        return Person {
                            name: Some(p1.to_string()),
                            url: Some(url),
                            ..Default::default()
                        };
                    }
                } else if p2.contains('@') {
                    return Person {
                        name: Some(p1.to_string()),
                        email: Some(p2.to_string()),
                        ..Default::default()
                    };
                }
                return Person {
                    name: Some(text.to_string()),
                    ..Default::default()
                };
            }
        } else if text.contains('<') {
            if let Some((name, email)) = parseaddr(text.as_str()) {
                return Person {
                    name: Some(name),
                    email: Some(email),
                    ..Default::default()
                };
            }
        }

        Person {
            name: Some(text.to_string()),
            ..Default::default()
        }
    }
}

fn parseaddr(text: &str) -> Option<(String, String)> {
    let re = Regex::new(r"(.*?)\s*<([^<>]+)>").unwrap();
    if let Some(captures) = re.captures(text) {
        let name = captures.get(1).map(|m| m.as_str().trim().to_string());
        let email = captures.get(2).map(|m| m.as_str().trim().to_string());
        if let (Some(name), Some(email)) = (name, email) {
            return Some((name, email));
        }
    }
    None
}

impl FromPyObject<'_> for Person {
    fn extract(ob: &'_ PyAny) -> PyResult<Self> {
        let name = ob.getattr("name")?.extract::<Option<String>>()?;
        let email = ob.getattr("email")?.extract::<Option<String>>()?;
        let url = ob.getattr("url")?.extract::<Option<String>>()?;
        Ok(Person { name, email, url })
    }
}

pub enum UpstreamDatum {
    Name(String),
    Homepage(String),
    Repository(String),
    RepositoryBrowse(String),
    Description(String),
    Summary(String),
    License(String),
    Author(Vec<Person>),
    Maintainer(Person),
    BugDatabase(String),
    BugSubmit(String),
    Contact(String),
    CargoCrate(String),
    SecurityMD(String),
    SecurityContact(String),
    Version(String),
    Keywords(Vec<String>),
    Copyright(String),
}

pub struct UpstreamDatumWithMetadata {
    pub datum: UpstreamDatum,
    pub origin: Option<String>,
    pub certainty: Option<Certainty>,
}

impl UpstreamDatum {
    pub fn field(&self) -> &'static str {
        match self {
            UpstreamDatum::Summary(..) => "Summary",
            UpstreamDatum::Description(..) => "Description",
            UpstreamDatum::Name(..) => "Name",
            UpstreamDatum::Homepage(..) => "Homepage",
            UpstreamDatum::Repository(..) => "Repository",
            UpstreamDatum::RepositoryBrowse(..) => "Repository-Browse",
            UpstreamDatum::License(..) => "License",
            UpstreamDatum::Author(..) => "Author",
            UpstreamDatum::BugDatabase(..) => "Bug-Database",
            UpstreamDatum::BugSubmit(..) => "Bug-Submit",
            UpstreamDatum::Contact(..) => "Contact",
            UpstreamDatum::CargoCrate(..) => "Cargo-Crate",
            UpstreamDatum::SecurityMD(..) => "Security-MD",
            UpstreamDatum::SecurityContact(..) => "Security-Contact",
            UpstreamDatum::Version(..) => "Version",
            UpstreamDatum::Keywords(..) => "Keywords",
            UpstreamDatum::Maintainer(..) => "Maintainer",
            UpstreamDatum::Copyright(..) => "Copyright",
        }
    }
}

pub fn guess_upstream_metadata(
    path: PathBuf,
    trust_package: Option<bool>,
    net_access: Option<bool>,
    consult_external_directory: Option<bool>,
    check: Option<bool>,
) -> Result<Vec<UpstreamDatum>, ()> {
    Python::with_gil(|py| {
        let guess = py.import("upstream_ontologist.guess").unwrap();
        let guess_upstream_metadata = guess.getattr("guess_upstream_metadata").unwrap();

        let items = guess_upstream_metadata
            .call1((
                path,
                trust_package,
                net_access,
                consult_external_directory,
                check,
            ))
            .unwrap()
            .extract::<HashMap<String, PyObject>>()
            .unwrap();

        let mut ret = Vec::new();
        for (name, value) in items.into_iter() {
            if value.is_none(py) {
                continue;
            }
            let entry = match name.as_str() {
                "Homepage" => UpstreamDatum::Homepage(value.extract::<String>(py).unwrap()),
                "Name" => UpstreamDatum::Name(value.extract::<String>(py).unwrap()),
                "Repository" => UpstreamDatum::Repository(value.extract::<String>(py).unwrap()),
                "Repository-Browse" => {
                    UpstreamDatum::RepositoryBrowse(value.extract::<String>(py).unwrap())
                }
                "Bug-Database" => UpstreamDatum::BugDatabase(value.extract::<String>(py).unwrap()),
                "Bug-Submit" => UpstreamDatum::BugSubmit(value.extract::<String>(py).unwrap()),
                "Contact" => UpstreamDatum::Contact(value.extract::<String>(py).unwrap()),
                "X-Security-MD" => UpstreamDatum::SecurityMD(value.extract::<String>(py).unwrap()),
                "Security-Contact" => {
                    UpstreamDatum::SecurityContact(value.extract::<String>(py).unwrap())
                }
                "Keywords" => UpstreamDatum::Keywords(value.extract::<Vec<String>>(py).unwrap()),
                "X-Cargo-Crate" => UpstreamDatum::CargoCrate(value.extract::<String>(py).unwrap()),
                "X-Description" => UpstreamDatum::Description(value.extract::<String>(py).unwrap()),
                "X-Summary" => UpstreamDatum::Summary(value.extract::<String>(py).unwrap()),
                "X-License" => UpstreamDatum::License(value.extract::<String>(py).unwrap()),
                "X-Version" => UpstreamDatum::Version(value.extract::<String>(py).unwrap()),
                "X-Author" => UpstreamDatum::Author(
                    value
                        .extract::<Vec<Person>>(py)
                        .unwrap()
                        .into_iter()
                        .collect(),
                ),
                _ => {
                    panic!("{}: {:?}", name, value);
                }
            };
            ret.push(entry);
        }
        Ok(ret)
    })
}

pub trait UpstreamDataProvider {
    fn provide(
        path: &std::path::Path,
        trust_package: bool,
    ) -> dyn Iterator<Item = (UpstreamDatum, Certainty)>;
}

pub fn url_from_git_clone_command(command: &[u8]) -> Option<String> {
    if command.ends_with(&[b'\\']) {
        warn!("Ignoring command with line break: {:?}", command);
        return None;
    }
    let command_str = match String::from_utf8(command.to_vec()) {
        Ok(s) => s,
        Err(_) => {
            warn!("Ignoring command with non-UTF-8: {:?}", command);
            return None;
        }
    };
    let argv: Vec<String> = shlex::split(command_str.as_str())?
        .into_iter()
        .filter(|arg| !arg.trim().is_empty())
        .collect();
    let mut args = argv;
    let mut i = 0;
    while i < args.len() {
        if !args[i].starts_with('-') {
            i += 1;
            continue;
        }
        if args[i].contains('=') {
            args.remove(i);
            continue;
        }
        // arguments that take a parameter
        if args[i] == "-b" || args[i] == "--depth" || args[i] == "--branch" {
            args.remove(i);
            args.remove(i);
            continue;
        }
        args.remove(i);
    }
    let url = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| args.get(0).cloned().unwrap_or_default());
    if vcs::plausible_url(&url) {
        Some(url)
    } else {
        None
    }
}

pub fn url_from_fossil_clone_command(command: &[u8]) -> Option<String> {
    let command_str = match String::from_utf8(command.to_vec()) {
        Ok(s) => s,
        Err(_) => return None,
    };
    let argv: Vec<String> = shlex::split(command_str.as_str())?
        .into_iter()
        .filter(|arg| !arg.trim().is_empty())
        .collect();
    let mut args = argv;
    let mut i = 0;
    while i < args.len() {
        if !args[i].starts_with('-') {
            i += 1;
            continue;
        }
        if args[i].contains('=') {
            args.remove(i);
            continue;
        }
        args.remove(i);
    }
    let url = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| args.get(0).cloned().unwrap_or_default());
    if vcs::plausible_url(&url) {
        Some(url)
    } else {
        None
    }
}

/*
pub fn url_from_cvs_co_command(command: &[u8]) -> Option<String> {
    let command_str = match String::from_utf8(command.to_vec()) {
        Ok(s) => s,
        Err(_) => return None,
    };
    let argv: Vec<String> = shlex::split(command_str.as_str())?
        .into_iter()
        .filter(|arg| !arg.trim().is_empty())
        .collect();
    let mut args = argv;
    let mut i = 0;
    let mut cvsroot = None;
    let mut module = None;
    let mut command_seen = false;
    args.remove(0);
    while i < args.len() {
        if args[i] == "-d" {
            args.remove(i);
            cvsroot = Some(&args[i][..]);
            args.remove(i);
            continue;
        }
        if args[i].starts_with("-d") {
            cvsroot = Some(&args[i][2..]);
            args.remove(i);
            continue;
        }
        if command_seen && !args[i].starts_with('-') {
            module = Some(args[i]);
        } else if args[i] == "co" || args[i] == "checkout" {
            command_seen = true;
        }
        args.remove(i);
    }
    if let Some(cvsroot) = cvsroot {
        let url = cvs_to_url(&cvsroot);
        if let Some(module) = module {
            return Some(url.join(module));
        }
        return Some(url);
    }
    None
}

*/

pub fn url_from_svn_co_command(command: &[u8]) -> Option<String> {
    if command.ends_with(&[b'\\']) {
        warn!("Ignoring command with line break: {:?}", command);
        return None;
    }
    let command_str = match std::str::from_utf8(command) {
        Ok(s) => s,
        Err(_) => return None,
    };
    let argv: Vec<String> = shlex::split(command_str)?
        .into_iter()
        .filter(|arg| !arg.trim().is_empty())
        .collect();
    let args = argv;
    let url_schemes = vec!["svn+ssh", "http", "https", "svn"];
    args.into_iter().find(|arg| {
        url_schemes
            .iter()
            .any(|scheme| arg.starts_with(&format!("{}://", scheme)))
    })
}

pub fn guess_from_meson(
    path: &std::path::Path,
    trust_package: bool,
) -> Vec<UpstreamDatumWithMetadata> {
    // TODO(jelmer): consider looking for a meson build directory to call "meson
    // introspect" on
    // TODO(jelmer): mesonbuild is python; consider using its internal functions to parse
    // meson.build?

    let mut command = Command::new("meson");
    command.arg("introspect").arg("--projectinfo").arg(path);
    let output = match command.output() {
        Ok(output) => output,
        Err(_) => {
            warn!("meson not installed; skipping meson.build introspection");
            return Vec::new();
        }
    };
    if !output.status.success() {
        warn!(
            "meson failed to run; exited with code {}",
            output.status.code().unwrap()
        );
        return Vec::new();
    }
    let project_info: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(_) => {
            warn!("Failed to parse meson project info");
            return Vec::new();
        }
    };
    let mut results = Vec::new();
    if let Some(descriptive_name) = project_info.get("descriptive_name") {
        if let Some(name) = descriptive_name.as_str() {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(name.to_owned()),
                certainty: Some(Certainty::Certain),
                origin: Some("meson.build".to_owned()),
            });
        }
    }
    if let Some(version) = project_info.get("version") {
        if let Some(version_str) = version.as_str() {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Version(version_str.to_owned()),
                certainty: Some(Certainty::Certain),
                origin: Some("meson.build".to_owned()),
            });
        }
    }
    results
}

pub fn guess_from_package_json(path: &Path, trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    // see https://docs.npmjs.com/cli/v7/configuring-npm/package-json
    let file = std::fs::File::open(path).expect("Failed to open package.json");
    let package: serde_json::Value =
        serde_json::from_reader(file).expect("Failed to parse package.json");

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();

    for (field, value) in package.as_object().unwrap() {
        match field.as_str() {
            "name" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some("package.json".to_string()),
                });
            }
            "homepage" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Homepage(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some("package.json".to_string()),
                });
            }
            "description" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some("package.json".to_string()),
                });
            }
            "license" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some("package.json".to_string()),
                });
            }
            "version" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some("package.json".to_string()),
                });
            }
            "repository" => {
                let repo_url = if let Some(repo_url) = value.as_str() {
                    Some(repo_url)
                } else if let Some(repo) = value.as_object() {
                    if let Some(repo_url) = repo.get("url") {
                        repo_url.as_str()
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(repo_url) = repo_url {
                    match Url::parse(repo_url) {
                        Ok(url) if url.scheme() == "github" => {
                            // Some people seem to default to github. :(
                            let repo_url = format!("https://github.com/{}", url.path());
                            upstream_data.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::Repository(repo_url.to_string()),
                                certainty: Some(Certainty::Likely),
                                origin: Some("package.json".to_string()),
                            });
                        }
                        Err(e) if e == url::ParseError::RelativeUrlWithoutBase => {
                            // Some people seem to default to github. :(
                            let repo_url = format!("https://github.com/{}", repo_url);
                            upstream_data.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::Repository(repo_url.to_string()),
                                certainty: Some(Certainty::Likely),
                                origin: Some("package.json".to_string()),
                            });
                        }
                        Ok(url) => {
                            upstream_data.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::Repository(url.to_string()),
                                certainty: Some(Certainty::Certain),
                                origin: Some("package.json".to_string()),
                            });
                        }
                        Err(e) => {
                            panic!("Failed to parse repository URL: {}", e);
                        }
                    }
                }
            }
            "bugs" => {
                if let Some(url) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::BugDatabase(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some("package.json".to_string()),
                    });
                } else if let Some(email) = value.get("email").and_then(serde_json::Value::as_str) {
                    let url = format!("mailto:{}", email);
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::BugDatabase(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some("package.json".to_string()),
                    });
                }
            }
            "author" => {
                if let Some(author) = value.as_object() {
                    let name = author
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .map(String::from);
                    let url = author
                        .get("url")
                        .and_then(serde_json::Value::as_str)
                        .map(String::from);
                    let email = author
                        .get("email")
                        .and_then(serde_json::Value::as_str)
                        .map(String::from);
                    let person = Person { name, url, email };
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Author(vec![person]),
                        certainty: Some(Certainty::Confident),
                        origin: Some("package.json".to_string()),
                    });
                } else if let Some(author) = value.as_str() {
                    let person = Person::from(author);
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Author(vec![person]),
                        certainty: Some(Certainty::Confident),
                        origin: Some("package.json".to_string()),
                    });
                } else {
                    error!("Unsupported type for author in package.json: {:?}", value);
                }
            }
            "dependencies" | "private" | "devDependencies" | "scripts" => {
                // Do nothing, skip these fields
            }
            _ => {
                error!("Unknown package.json field {} ({:?})", field, value);
            }
        }
    }

    upstream_data
}

pub fn debian_is_native(path: &Path) -> std::io::Result<Option<bool>> {
    let format_file_path = path.join("source/format");
    match File::open(format_file_path) {
        Ok(mut file) => {
            let mut content = String::new();
            file.read_to_string(&mut content)?;
            Ok(Some(content.trim() == "3.0 (native)"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub enum HTTPJSONError {
    HTTPError(reqwest::Error),
    Error {
        url: reqwest::Url,
        status: u16,
        response: reqwest::blocking::Response,
    },
}

pub fn load_json_url(
    http_url: &str,
    timeout: Option<std::time::Duration>,
) -> Result<serde_json::Value, HTTPJSONError> {
    let timeout = timeout.unwrap_or(std::time::Duration::from_secs(DEFAULT_URLLIB_TIMEOUT));
    let mut headers = HeaderMap::new();
    headers.insert(reqwest::header::USER_AGENT, USER_AGENT.parse().unwrap());
    headers.insert(reqwest::header::ACCEPT, "application/json".parse().unwrap());

    if let Some(hostname) = reqwest::Url::parse(http_url).unwrap().host_str() {
        if hostname == "github.com" || hostname == "raw.githubusercontent.com" {
            if let Ok(token) = std::env::var("GITHUB_TOKEN") {
                headers.insert(
                    reqwest::header::WWW_AUTHENTICATE,
                    format!("Bearer {}", token).parse().unwrap(),
                );
            }
        }
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .default_headers(headers)
        .build()
        .map_err(HTTPJSONError::HTTPError)?;

    let request = client
        .get(http_url)
        .build()
        .map_err(HTTPJSONError::HTTPError)?;

    let response = client.execute(request).map_err(HTTPJSONError::HTTPError)?;

    if !response.status().is_success() {
        return Err(HTTPJSONError::Error {
            url: response.url().clone(),
            status: response.status().as_u16(),
            response,
        });
    }

    let json_contents: serde_json::Value = response.json().map_err(HTTPJSONError::HTTPError)?;

    Ok(json_contents)
}

pub fn guess_from_composer_json(
    path: &Path,
    trust_package: bool,
) -> Vec<UpstreamDatumWithMetadata> {
    // https://getcomposer.org/doc/04-schema.md
    let file = std::fs::File::open(path).expect("Failed to open composer.json");
    let package: serde_json::Value =
        serde_json::from_reader(file).expect("Failed to parse composer.json");

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();

    for (field, value) in package.as_object().unwrap() {
        match field.as_str() {
            "name" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some("composer.json".to_string()),
                });
            }
            "homepage" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Homepage(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some("composer.json".to_string()),
                });
            }
            "description" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some("composer.json".to_string()),
                });
            }
            "license" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some("composer.json".to_string()),
                });
            }
            "version" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(value.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some("composer.json".to_string()),
                });
            }
            "type" => {
                if value != "project" {
                    error!("unexpected composer.json type: {:?}", value);
                }
            }
            "keywords" => {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Keywords(
                        value
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|v| v.as_str().unwrap().to_string())
                            .collect(),
                    ),
                    certainty: Some(Certainty::Certain),
                    origin: Some("composer.json".to_string()),
                });
            }
            "require" | "require-dev" | "autoload" | "autoload-dev" | "scripts" | "extra"
            | "config" | "prefer-stable" | "minimum-stability" => {
                // Do nothing, skip these fields
            }
            _ => {
                error!("Unknown field {} ({:?}) in composer.json", field, value);
            }
        }
    }

    upstream_data
}

fn xmlparse_simplify_namespaces(path: &Path, namespaces: &[&str]) -> Option<Element> {
    let namespaces = namespaces
        .iter()
        .map(|ns| format!("{{{}{}}}", ns, ns))
        .collect::<Vec<_>>();
    let mut f = std::fs::File::open(path).unwrap();
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let mut tree = xmltree::Element::parse(std::io::Cursor::new(buf)).ok()?;
    simplify_namespaces(&mut tree, &namespaces);
    Some(tree)
}

fn simplify_namespaces(element: &mut Element, namespaces: &[String]) {
    element.prefix = None;
    if let Some(namespace) = namespaces.iter().find(|&ns| element.name.starts_with(ns)) {
        element.name = element.name[namespace.len()..].to_string();
    }
    for child in &mut element.children {
        if let XMLNode::Element(ref mut child_element) = child {
            simplify_namespaces(child_element, namespaces);
        }
    }
}

pub fn guess_from_package_xml(path: &Path, trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    let namespaces = [
        "http://pear.php.net/dtd/package-2.0",
        "http://pear.php.net/dtd/package-2.1",
    ];

    let root = match xmlparse_simplify_namespaces(path, &namespaces) {
        Some(root) => root,
        None => {
            eprintln!("Unable to parse package.xml");
            return Vec::new();
        }
    };

    assert_eq!(root.name, "package", "root tag is {:?}", root.name);

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();
    let mut leads: Vec<&Element> = Vec::new();

    for child_element in &root.children {
        if let XMLNode::Element(ref element) = child_element {
            match element.name.as_str() {
                "name" => {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Name(element.get_text().unwrap().to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some("package.xml".to_string()),
                    });
                }
                "summary" => {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Summary(element.get_text().unwrap().to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some("package.xml".to_string()),
                    });
                }
                "description" => {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Description(element.get_text().unwrap().to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some("package.xml".to_string()),
                    });
                }
                "version" => {
                    if let Some(release_tag) = element.get_child("release") {
                        upstream_data.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Version(
                                release_tag.get_text().unwrap().to_string(),
                            ),
                            certainty: Some(Certainty::Certain),
                            origin: Some("package.xml".to_string()),
                        });
                    }
                }
                "license" => {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::License(element.get_text().unwrap().to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some("package.xml".to_string()),
                    });
                }
                "url" => {
                    if let Some(url_type) = element.attributes.get("type") {
                        match url_type.as_str() {
                            "repository" => {
                                upstream_data.push(UpstreamDatumWithMetadata {
                                    datum: UpstreamDatum::Repository(
                                        element.get_text().unwrap().to_string(),
                                    ),
                                    certainty: Some(Certainty::Certain),
                                    origin: Some("package.xml".to_string()),
                                });
                            }
                            "bugtracker" => {
                                upstream_data.push(UpstreamDatumWithMetadata {
                                    datum: UpstreamDatum::BugDatabase(
                                        element.get_text().unwrap().to_string(),
                                    ),
                                    certainty: Some(Certainty::Certain),
                                    origin: Some("package.xml".to_string()),
                                });
                            }
                            _ => {}
                        }
                    }
                }
                "lead" => {
                    leads.push(element);
                }
                "stability" | "dependencies" | "providesextension" | "extsrcrelease"
                | "channel" | "notes" | "contents" | "date" | "time" => {
                    // Do nothing, skip these fields
                }
                _ => {
                    eprintln!("Unknown package.xml tag {}", element.name);
                }
            }
        }
    }

    for lead_element in leads.iter().take(1) {
        let name_el = lead_element.get_child("name");
        let email_el = lead_element.get_child("email");
        let active_el = lead_element.get_child("active");
        if let Some(active_el) = active_el {
            if active_el.get_text().as_deref() != Some("yes") {
                continue;
            }
        }
        let person = Person {
            name: name_el.map(|el| el.get_text().unwrap().into_owned()),
            email: email_el.map(|el| el.get_text().unwrap().into_owned()),
            ..Default::default()
        };
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Maintainer(person),
            certainty: Some(Certainty::Confident),
            origin: Some("package.xml".to_string()),
        });
    }

    upstream_data
}

pub fn guess_from_pod(contents: &str) -> Vec<UpstreamDatumWithMetadata> {
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
        description = Regex::new(r"[FXZSCBI]\\<([^>]+)>")
            .unwrap()
            .replace_all(&description, "$1")
            .into_owned();
        description = Regex::new(r"L\\<([^\|]+)\|([^\\>]+)\\>")
            .unwrap()
            .replace_all(&description, "$2")
            .into_owned();
        description = Regex::new(r"L\\<([^\\>]+)\\>")
            .unwrap()
            .replace_all(&description, "$1")
            .into_owned();

        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Description(description),
            certainty: Some(Certainty::Certain),
            origin: Some("pod".to_string()),
        });
    }

    if let Some(name) = by_header.get("NAME") {
        let lines: Vec<&str> = name.trim().lines().collect();
        if let Some(line) = lines.first() {
            if let Some((name, summary)) = line.split_once(" - ") {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(name.trim().to_string()),
                    certainty: Some(Certainty::Confident),
                    origin: Some("pod".to_string()),
                });
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(summary.trim().to_string()),
                    certainty: Some(Certainty::Confident),
                    origin: Some("pod".to_string()),
                });
            } else if !line.contains(' ') {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(line.trim().to_string()),
                    certainty: Some(Certainty::Confident),
                    origin: Some("pod".to_string()),
                });
            }
        }
    }

    upstream_data
}

pub fn guess_from_perl_module(path: &Path) -> Vec<UpstreamDatumWithMetadata> {
    match Command::new("perldoc").arg("-u").arg(path).output() {
        Ok(output) => guess_from_pod(&String::from_utf8_lossy(&output.stdout)),
        Err(_) => {
            error!("Error running perldoc, skipping.");
            Vec::new()
        }
    }
}

pub fn guess_from_perl_dist_name(path: &Path, dist_name: &str) -> Vec<UpstreamDatumWithMetadata> {
    let mod_path = PathBuf::from(format!(
        "{}/lib/{}.pm",
        std::path::Path::new(path).parent().unwrap().display(),
        dist_name.replace('-', "/")
    ));

    if mod_path.exists() {
        guess_from_perl_module(mod_path.as_path())
    } else {
        Vec::new()
    }
}

pub fn guess_from_dist_ini(path: &Path, trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    let parser = match ini::Ini::load_from_file(path) {
        Err(e) => {
            error!("Error parsing dist.ini: {}", e);
            return Vec::new();
        }
        Ok(parser) => parser,
    };

    let dist_name = parser
        .get_from::<&str>(None, "name")
        .map(|name| UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(name.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some("dist.ini".to_string()),
        });

    let version =
        parser
            .get_from::<&str>(None, "version")
            .map(|version| UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Version(version.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some("dist.ini".to_string()),
            });

    let summary =
        parser
            .get_from::<&str>(None, "abstract")
            .map(|summary| UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Summary(summary.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some("dist.ini".to_string()),
            });

    let bug_database = parser
        .get_from(Some("MetaResources"), "bugtracker.web")
        .map(|bugtracker| UpstreamDatumWithMetadata {
            datum: UpstreamDatum::BugDatabase(bugtracker.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some("dist.ini".to_string()),
        });

    let repository = parser
        .get_from(Some("MetaResources"), "repository.url")
        .map(|repository| UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository(repository.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some("dist.ini".to_string()),
        });

    let license =
        parser
            .get_from::<&str>(None, "license")
            .map(|license| UpstreamDatumWithMetadata {
                datum: UpstreamDatum::License(license.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some("dist.ini".to_string()),
            });

    let copyright = match (
        parser.get_from::<&str>(None, "copyright_year"),
        parser.get_from::<&str>(None, "copyright_holder"),
    ) {
        (Some(year), Some(holder)) => Some(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Copyright(format!("{} {}", year, holder)),
            certainty: Some(Certainty::Certain),
            origin: Some("dist.ini".to_string()),
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
        upstream_data.extend(guess_from_perl_dist_name(path, dist_name));
    }

    upstream_data
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_guess_upstream_metadata() {
        guess_upstream_metadata(
            PathBuf::from("."),
            Some(true),
            Some(true),
            Some(true),
            Some(true),
        )
        .unwrap();
    }

    #[test]
    fn test_url_from_git_clone_command() {
        assert_eq!(
            url_from_git_clone_command(b"git clone https://github.com/foo/bar foo"),
            Some("https://github.com/foo/bar".to_string())
        );
    }
}
