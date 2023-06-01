use log::{debug, error, warn};
use percent_encoding::utf8_percent_encode;
use pyo3::prelude::*;
use regex::Regex;
use reqwest::header::HeaderMap;
use reqwest::IntoUrl;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
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
    Documentation(String),
    GoImportPath(String),
    Download(String),
    Wiki(String),
    MailingList(String),
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
            UpstreamDatum::Documentation(..) => "Documentation",
            UpstreamDatum::GoImportPath(..) => "Go-Import-Path",
            UpstreamDatum::Download(..) => "Download",
            UpstreamDatum::Wiki(..) => "Wiki",
            UpstreamDatum::MailingList(..) => "MailingList",
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
                "Documentation" => {
                    UpstreamDatum::Documentation(value.extract::<String>(py).unwrap())
                }
                "Copyright" => UpstreamDatum::Copyright(value.extract::<String>(py).unwrap()),
                "Keywords" => UpstreamDatum::Keywords(value.extract::<Vec<String>>(py).unwrap()),
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

#[derive(Debug)]
pub enum HTTPJSONError {
    HTTPError(reqwest::Error),
    Error {
        url: reqwest::Url,
        status: u16,
        response: reqwest::blocking::Response,
    },
}

pub fn load_json_url(
    http_url: &Url,
    timeout: Option<std::time::Duration>,
) -> Result<serde_json::Value, HTTPJSONError> {
    let timeout = timeout.unwrap_or(std::time::Duration::from_secs(DEFAULT_URLLIB_TIMEOUT));
    let mut headers = HeaderMap::new();
    headers.insert(reqwest::header::USER_AGENT, USER_AGENT.parse().unwrap());
    headers.insert(reqwest::header::ACCEPT, "application/json".parse().unwrap());

    if let Some(hostname) = http_url.host_str() {
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

    let http_url: reqwest::Url = http_url.clone().into_string().parse().unwrap();

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
            error!("Unable to parse package.xml");
            return Vec::new();
        }
    };

    assert_eq!(root.name, "package", "root tag is {:?}", root.name);

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();
    let mut leads: Vec<&Element> = Vec::new();
    let mut maintainers: Vec<&Element> = Vec::new();
    let mut authors: Vec<&Element> = Vec::new();

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
                "maintainer" => {
                    maintainers.push(element);
                }
                "author" => {
                    authors.push(element);
                }
                "stability" | "dependencies" | "providesextension" | "extsrcrelease"
                | "channel" | "notes" | "contents" | "date" | "time" => {
                    // Do nothing, skip these fields
                }
                _ => {
                    error!("Unknown package.xml tag {}", element.name);
                }
            }
        }
    }

    for lead_element in leads.iter().take(1) {
        let name_el = lead_element.get_child("name").unwrap().get_text();
        let email_el = lead_element
            .get_child("email")
            .map(|s| s.get_text().unwrap());
        let active_el = lead_element
            .get_child("active")
            .map(|s| s.get_text().unwrap());
        if let Some(active_el) = active_el {
            if active_el != "yes" {
                continue;
            }
        }
        let person = Person {
            name: name_el.map(|s| s.to_string()),
            email: email_el.map(|s| s.to_string()),
            ..Default::default()
        };
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Maintainer(person),
            certainty: Some(Certainty::Confident),
            origin: Some("package.xml".to_string()),
        });
    }

    if maintainers.len() == 1 {
        let maintainer_element = maintainers[0];
        let name_el = maintainer_element.get_text().map(|s| s.into_owned());
        let email_el = maintainer_element.attributes.get("email");
        let person = Person {
            name: name_el,
            email: email_el.map(|s| s.to_string()),
            ..Default::default()
        };
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Maintainer(person),
            certainty: Some(Certainty::Confident),
            origin: Some("package.xml".to_string()),
        });
    }

    if !authors.is_empty() {
        let persons = authors
            .iter()
            .map(|author_element| {
                let name_el = author_element.get_text().unwrap().into_owned();
                let email_el = author_element.attributes.get("email");
                Person {
                    name: Some(name_el),
                    email: email_el.map(|s| s.to_string()),
                    ..Default::default()
                }
            })
            .collect();
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Author(persons),
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

#[derive(serde::Deserialize)]
struct Pubspec {
    name: Option<String>,
    description: Option<String>,
    version: Option<String>,
    homepage: Option<String>,
    repository: Option<String>,
    documentation: Option<String>,
    issue_tracker: Option<String>,
}

pub fn guess_from_pubspec_yaml(path: &Path, trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    let file = File::open(path).unwrap();

    let pubspec: Pubspec = match serde_yaml::from_reader(file) {
        Ok(pubspec) => pubspec,
        Err(e) => {
            error!("Unable to parse {}: {}", path.display(), e);
            return Vec::new();
        }
    };

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();

    if let Some(name) = pubspec.name {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(name),
            certainty: Some(Certainty::Certain),
            origin: Some("pubspec.yaml".to_string()),
        });
    }
    if let Some(description) = pubspec.description {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Description(description),
            certainty: Some(Certainty::Certain),
            origin: Some("pubspec.yaml".to_string()),
        });
    }
    if let Some(version) = pubspec.version {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Version(version),
            certainty: Some(Certainty::Certain),
            origin: Some("pubspec.yaml".to_string()),
        });
    }
    if let Some(homepage) = pubspec.homepage {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Homepage(homepage),
            certainty: Some(Certainty::Certain),
            origin: Some("pubspec.yaml".to_string()),
        });
    }
    if let Some(repository) = pubspec.repository {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository(repository),
            certainty: Some(Certainty::Certain),
            origin: Some("pubspec.yaml".to_string()),
        });
    }
    if let Some(documentation) = pubspec.documentation {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Documentation(documentation),
            certainty: Some(Certainty::Certain),
            origin: Some("pubspec.yaml".to_string()),
        });
    }
    if let Some(issue_tracker) = pubspec.issue_tracker {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::BugDatabase(issue_tracker),
            certainty: Some(Certainty::Certain),
            origin: Some("pubspec.yaml".to_string()),
        });
    }

    upstream_data
}

pub fn guess_from_authors(path: &Path, trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    let file = File::open(path).unwrap();
    let reader = std::io::BufReader::new(file);

    let mut authors: Vec<Person> = Vec::new();

    for line in reader.lines() {
        if let Ok(line) = line {
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
    }

    vec![UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Author(authors),
        certainty: Some(Certainty::Likely),
        origin: Some(path.to_string_lossy().to_string()),
    }]
}

pub fn guess_from_metadata_json(
    path: &Path,
    trust_package: bool,
) -> Vec<UpstreamDatumWithMetadata> {
    let mut file = File::open(path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();

    let data: serde_json::Map<String, serde_json::Value> = match serde_json::from_str(&contents) {
        Ok(data) => data,
        Err(e) => {
            error!("Unable to parse {}: {}", path.display(), e);
            return Vec::new();
        }
    };

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();

    for (field, value) in data.iter() {
        match field.as_str() {
            "description" => {
                if let Some(description) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Description(description.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            "name" => {
                if let Some(name) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Name(name.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            "version" => {
                if let Some(version) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Version(version.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            "url" => {
                if let Some(url) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Homepage(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            "license" => {
                if let Some(license) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::License(license.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            "source" => {
                if let Some(repository) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Repository(repository.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            "summary" => {
                if let Some(summary) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Summary(summary.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            "issues_url" => {
                if let Some(issues_url) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::BugDatabase(issues_url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            "project_page" => {
                if let Some(project_page) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Homepage(project_page.to_string()),
                        certainty: Some(Certainty::Likely),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            "author" => {
                if let Some(author_value) = value.as_str() {
                    let author = Person::from(author_value);
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Author(vec![author]),
                        certainty: Some(Certainty::Likely),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                } else if let Some(author_values) = value.as_array() {
                    let authors: Vec<Person> = author_values
                        .iter()
                        .map(|v| Person::from(v.as_str().unwrap()))
                        .collect();
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Author(authors),
                        certainty: Some(Certainty::Likely),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            "operatingsystem_support" | "requirements" | "dependencies" => {
                // Skip these fields
            }
            _ => {
                warn!("Unknown field {} ({:?}) in metadata.json", field, value);
            }
        }
    }

    upstream_data
}

pub fn guess_from_meta_json(path: &Path, trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    let mut file = File::open(path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();

    let data: serde_json::Map<String, serde_json::Value> = match serde_json::from_str(&contents) {
        Ok(data) => data,
        Err(e) => {
            error!("Unable to parse {}: {}", path.display(), e);
            return Vec::new();
        }
    };

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();

    if let Some(name) = data.get("name").and_then(serde_json::Value::as_str) {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(name.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.to_string_lossy().to_string()),
        });
    }

    if let Some(version) = data.get("version").and_then(serde_json::Value::as_str) {
        let version = version.strip_prefix('v').unwrap_or(version);
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Version(version.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.to_string_lossy().to_string()),
        });
    }

    if let Some(summary) = data.get("abstract").and_then(serde_json::Value::as_str) {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Summary(summary.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.to_string_lossy().to_string()),
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
                    origin: Some(path.to_string_lossy().to_string()),
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
                origin: Some(path.to_string_lossy().to_string()),
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
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }

            if let Some(web) = repo.get("web").and_then(serde_json::Value::as_str) {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::RepositoryBrowse(web.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
        }
    }

    // Wild guess:
    if let Some(dist_name) = data.get("name").and_then(serde_json::Value::as_str) {
        upstream_data.extend(guess_from_perl_dist_name(path, dist_name));
    }

    upstream_data
}

/*
pub fn guess_from_debian_patch(path: &Path, trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    let file = File::open(path).unwrap();
    let reader = std::io::BufReader::new(file);

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();

    for line in reader.lines() {
        if let Ok(line) = line {
            if line.starts_with("Forwarded: ") {
                let forwarded = line.splitn(2, ':').nth(1).unwrap().trim();
                let forwarded_str = forwarded.to_string();

                if let Some(bug_db) = bug_database_from_issue_url(&forwarded_str) {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::BugDatabase(bug_db),
                        certainty: Some(Certainty::Possible),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }

                if let Some(repo_url) = repo_url_from_merge_request_url(&forwarded_str) {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Repository(repo_url),
                        certainty: Some(Certainty::Possible),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
        }
    }

    upstream_data
}

*/

pub enum CanonicalizeError {
    InvalidUrl(Url, String),
    Unverifiable(Url, String),
    RateLimited(Url),
}

pub fn check_url_canonical(url: &Url) -> Result<Url, CanonicalizeError> {
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(CanonicalizeError::Unverifiable(
            url.clone(),
            format!("Unsupported scheme {}", url.scheme()),
        ));
    }

    let timeout = std::time::Duration::from_secs(DEFAULT_URLLIB_TIMEOUT);
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::USER_AGENT, USER_AGENT.parse().unwrap());
    let client = reqwest::blocking::Client::builder()
        .default_headers(headers)
        .timeout(timeout)
        .build()
        .unwrap();

    let response = client
        .get(url.clone())
        .send()
        .map_err(|e| CanonicalizeError::Unverifiable(url.clone(), format!("HTTP error {}", e)))?;

    match response.status() {
        status if status.is_success() => Ok(response.url().clone()),
        status if status == reqwest::StatusCode::TOO_MANY_REQUESTS => {
            Err(CanonicalizeError::RateLimited(url.clone()))
        }
        status if status == reqwest::StatusCode::NOT_FOUND => Err(CanonicalizeError::InvalidUrl(
            url.clone(),
            format!("Not found: {}", response.status()),
        )),
        status if status.is_server_error() => Err(CanonicalizeError::Unverifiable(
            url.clone(),
            format!("Server down: {}", response.status()),
        )),
        _ => Err(CanonicalizeError::Unverifiable(
            url.clone(),
            format!("Unknown HTTP error {}", response.status()),
        )),
    }
}

fn with_path_segments(url: &Url, path_segments: &[&str]) -> Result<Url, ()> {
    let mut url = url.clone();
    url.path_segments_mut()?
        .clear()
        .extend(path_segments.iter());
    Ok(url)
}

pub trait Forge: Send + Sync {
    fn repository_browse_can_be_homepage(&self) -> bool;

    fn name(&self) -> &'static str;

    fn bug_database_url_from_bug_submit_url(&self, url: &Url) -> Option<Url>;

    fn bug_submit_url_from_bug_database_url(&self, url: &Url) -> Option<Url>;

    fn check_bug_database_canonical(&self, url: &Url) -> Result<Url, CanonicalizeError>;

    fn check_bug_submit_url_canonical(&self, url: &Url) -> Result<Url, CanonicalizeError>;

    fn bug_database_from_issue_url(&self, url: &Url) -> Option<Url>;

    fn bug_database_url_from_repo_url(&self, url: &Url) -> Option<Url>;

    fn repo_url_from_merge_request_url(&self, url: &Url) -> Option<Url>;
}

pub struct GitHub;

impl GitHub {
    pub fn new() -> Self {
        Self
    }
}

impl Forge for GitHub {
    fn name(&self) -> &'static str {
        "GitHub"
    }

    fn repository_browse_can_be_homepage(&self) -> bool {
        true
    }

    fn bug_database_url_from_bug_submit_url(&self, url: &Url) -> Option<Url> {
        assert_eq!(url.host(), Some(url::Host::Domain("github.com")));
        let path_elements = url.path_segments().unwrap().collect::<Vec<_>>();

        if path_elements.len() != 3 && path_elements.len() != 4 {
            return None;
        }
        if path_elements[2] != "issues" {
            return None;
        }

        let mut url = url.clone();

        url.set_scheme("https").unwrap();

        Some(with_path_segments(&url, &path_elements[0..3]).unwrap())
    }

    fn bug_submit_url_from_bug_database_url(&self, url: &Url) -> Option<Url> {
        assert_eq!(url.host(), Some(url::Host::Domain("github.com")));
        let path_elements = url.path_segments().unwrap().collect::<Vec<_>>();

        if path_elements.len() != 3 {
            return None;
        }
        if path_elements[2] != "issues" {
            return None;
        }

        let mut url = url.clone();
        url.set_scheme("https").unwrap();
        url.path_segments_mut().unwrap().push("new");
        Some(url)
    }

    fn check_bug_database_canonical(&self, url: &Url) -> Result<Url, CanonicalizeError> {
        assert_eq!(url.host(), Some(url::Host::Domain("github.com")));
        let path_elements = url.path_segments().unwrap().collect::<Vec<_>>();

        if path_elements.len() != 3 {
            return Err(CanonicalizeError::InvalidUrl(
                url.clone(),
                "GitHub URL with missing path elements".to_string(),
            ));
        }
        if path_elements[2] != "issues" {
            return Err(CanonicalizeError::InvalidUrl(
                url.clone(),
                "GitHub URL with missing path elements".to_string(),
            ));
        }

        let api_url = Url::parse(&format!(
            "https://api.github.com/repos/{}/{}",
            path_elements[0], path_elements[1]
        ))
        .unwrap();

        let response = match reqwest::blocking::get(api_url) {
            Ok(response) => response,
            Err(e) if e.status() == Some(reqwest::StatusCode::NOT_FOUND) => {
                return Err(CanonicalizeError::InvalidUrl(
                    url.clone(),
                    format!("Project does not exist {}", e),
                ));
            }
            Err(e) if e.status() == Some(reqwest::StatusCode::FORBIDDEN) => {
                // Probably rate limited
                warn!("Unable to verify bug database URL {}: {}", url, e);
                return Err(CanonicalizeError::RateLimited(url.clone()));
            }
            Err(e) => {
                return Err(CanonicalizeError::Unverifiable(
                    url.clone(),
                    format!("Unable to verify bug database URL: {}", e),
                ));
            }
        };
        let data = response.json::<serde_json::Value>().unwrap();

        if data["has_issues"].as_bool() != Some(true) {
            return Err(CanonicalizeError::InvalidUrl(
                url.clone(),
                "Project does not have issues enabled".to_string(),
            ));
        }

        if data.get("archived").unwrap_or(&serde_json::Value::Null)
            == &serde_json::Value::Bool(true)
        {
            return Err(CanonicalizeError::InvalidUrl(
                url.clone(),
                "Project is archived".to_string(),
            ));
        }

        let mut url = Url::parse(data["html_url"].as_str().unwrap()).unwrap();

        url.set_scheme("https").unwrap();
        url.path_segments_mut().unwrap().push("issues");

        Ok(url)
    }

    fn check_bug_submit_url_canonical(&self, url: &Url) -> Result<Url, CanonicalizeError> {
        let mut path_segments = url.path_segments().unwrap().collect::<Vec<_>>();
        path_segments.pop();
        let db_url = with_path_segments(url, &path_segments).unwrap();
        let mut canonical_db_url = self.check_bug_database_canonical(&db_url)?;
        canonical_db_url.set_scheme("https").unwrap();
        canonical_db_url.path_segments_mut().unwrap().push("new");
        Ok(canonical_db_url)
    }

    fn bug_database_from_issue_url(&self, url: &Url) -> Option<Url> {
        let path_elements = url.path_segments().unwrap().collect::<Vec<_>>();
        if path_elements.len() < 2 || path_elements[1] != "issues" {
            return None;
        }
        let mut url = url.clone();
        url.set_scheme("https").unwrap();
        Some(with_path_segments(&url, &path_elements[0..3]).unwrap())
    }

    fn bug_database_url_from_repo_url(&self, url: &Url) -> Option<Url> {
        let mut path = url
            .path_segments()
            .into_iter()
            .take(2)
            .flatten()
            .collect::<Vec<&str>>();
        path[1] = path[1].strip_suffix(".git").unwrap_or(path[1]);
        path.push("issues");

        let mut url = url.clone();
        url.set_scheme("https").unwrap();
        Some(with_path_segments(&url, path.as_slice()).unwrap())
    }

    fn repo_url_from_merge_request_url(&self, url: &Url) -> Option<Url> {
        let path_elements = url.path_segments().unwrap().collect::<Vec<_>>();
        if path_elements.len() < 2 || path_elements[1] != "issues" {
            return None;
        }
        let mut url = url.clone();
        url.set_scheme("https").unwrap();
        Some(with_path_segments(&url, &path_elements[0..2]).unwrap())
    }
}

static DEFAULT_ASCII_SET: percent_encoding::AsciiSet = percent_encoding::CONTROLS
    .add(b'/')
    .add(b'?')
    .add(b'#')
    .add(b'%');

pub struct GitLab;

impl GitLab {
    pub fn new() -> Self {
        Self
    }
}

impl Forge for GitLab {
    fn name(&self) -> &'static str {
        "GitLab"
    }

    fn repository_browse_can_be_homepage(&self) -> bool {
        true
    }

    fn bug_database_url_from_bug_submit_url(&self, url: &Url) -> Option<Url> {
        let mut path_elements = url.path_segments().unwrap().collect::<Vec<_>>();

        if path_elements.len() < 2 {
            return None;
        }
        if path_elements[path_elements.len() - 2] != "issues" {
            return None;
        }
        if path_elements[path_elements.len() - 1] != "new" {
            path_elements.pop();
        }

        Some(with_path_segments(url, &path_elements[0..path_elements.len() - 3]).unwrap())
    }

    fn bug_submit_url_from_bug_database_url(&self, url: &Url) -> Option<Url> {
        let path_elements = url.path_segments().unwrap().collect::<Vec<_>>();

        if path_elements.len() < 2 {
            return None;
        }
        if path_elements[path_elements.len() - 1] != "issues" {
            return None;
        }

        let mut url = url.clone();
        url.path_segments_mut().unwrap().push("new");

        Some(url)
    }

    fn check_bug_database_canonical(&self, url: &Url) -> Result<Url, CanonicalizeError> {
        let host = url.host().unwrap();
        let mut path_elements = url.path_segments().unwrap().collect::<Vec<_>>();
        if path_elements.len() < 2 || path_elements[path_elements.len() - 1] != "issues" {
            return Err(CanonicalizeError::InvalidUrl(
                url.clone(),
                "GitLab URL with missing path elements".to_string(),
            ));
        }

        path_elements.pop();

        let proj = path_elements.join("/");
        let proj_segment = utf8_percent_encode(proj.as_str(), &DEFAULT_ASCII_SET);
        let api_url = Url::parse(&format!(
            "https://{}/api/v4/projects/{}",
            host, proj_segment
        ))
        .unwrap();
        match load_json_url(&api_url, None) {
            Ok(data) => {
                // issues_enabled is only provided when the user is authenticated,
                // so if we're not then we just fall back to checking the canonical URL
                let issues_enabled = data
                    .get("issues_enabled")
                    .unwrap_or(&serde_json::Value::Null);
                if issues_enabled.as_bool() == Some(false) {
                    return Err(CanonicalizeError::InvalidUrl(
                        url.clone(),
                        "Project does not have issues enabled".to_string(),
                    ));
                }

                let mut canonical_url = Url::parse(data["web_url"].as_str().unwrap()).unwrap();
                canonical_url
                    .path_segments_mut()
                    .unwrap()
                    .extend(&["-", "issues"]);
                if issues_enabled.as_bool() == Some(true) {
                    return Ok(canonical_url);
                }

                check_url_canonical(&canonical_url)
            }
            Err(HTTPJSONError::Error { status, .. })
                if status == reqwest::StatusCode::NOT_FOUND =>
            {
                Err(CanonicalizeError::InvalidUrl(
                    url.clone(),
                    "Project not found".to_string(),
                ))
            }
            Err(e) => Err(CanonicalizeError::Unverifiable(
                url.clone(),
                format!("Unable to verify bug database URL: {:?}", e),
            )),
        }
    }

    fn check_bug_submit_url_canonical(&self, url: &Url) -> Result<Url, CanonicalizeError> {
        let path_elements = url.path_segments().unwrap().collect::<Vec<_>>();
        if path_elements.len() < 2 || path_elements[path_elements.len() - 2] != "issues" {
            return Err(CanonicalizeError::InvalidUrl(
                url.clone(),
                "GitLab URL with missing path elements".to_string(),
            ));
        }

        if path_elements[path_elements.len() - 1] != "new" {
            return Err(CanonicalizeError::InvalidUrl(
                url.clone(),
                "GitLab URL with missing path elements".to_string(),
            ));
        }

        let db_url = with_path_segments(url, &path_elements[0..path_elements.len() - 1]).unwrap();
        let mut canonical_db_url = self.check_bug_database_canonical(&db_url)?;
        canonical_db_url.path_segments_mut().unwrap().push("new");
        Ok(canonical_db_url)
    }

    fn bug_database_from_issue_url(&self, url: &Url) -> Option<Url> {
        let path_elements = url.path_segments().unwrap().collect::<Vec<_>>();
        if path_elements.len() < 2
            || path_elements[path_elements.len() - 2] != "issues"
            || path_elements[path_elements.len() - 1]
                .parse::<u32>()
                .is_err()
        {
            return None;
        }
        Some(with_path_segments(url, &path_elements[0..path_elements.len() - 1]).unwrap())
    }

    fn bug_database_url_from_repo_url(&self, url: &Url) -> Option<Url> {
        let mut url = url.clone();
        let last = url.path_segments().unwrap().last().unwrap().to_string();
        url.path_segments_mut()
            .unwrap()
            .pop()
            .push(last.trim_end_matches(".git"))
            .push("issues");
        Some(url)
    }

    fn repo_url_from_merge_request_url(&self, url: &Url) -> Option<Url> {
        let path_elements = url.path_segments().unwrap().collect::<Vec<_>>();
        if path_elements.len() < 3
            || path_elements[path_elements.len() - 2] != "merge_requests"
            || path_elements[path_elements.len() - 1]
                .parse::<u32>()
                .is_err()
        {
            return None;
        }
        Some(with_path_segments(url, &path_elements[0..path_elements.len() - 2]).unwrap())
    }
}

pub fn guess_from_travis_yml(path: &Path, trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => {
            error!("Unable to open file: {}", path.display());
            return vec![];
        }
    };

    let mut contents = String::new();
    if let Err(_) = file.read_to_string(&mut contents) {
        error!("Unable to read file: {}", path.display());
        return vec![];
    }

    let data: serde_yaml::Value = match serde_yaml::from_str(&contents) {
        Ok(d) => d,
        Err(e) => {
            error!("Unable to parse YAML: {}", e);
            return vec![];
        }
    };

    if let Some(go_import_path) = data.get("go_import_path") {
        let upstream_datum = UpstreamDatumWithMetadata {
            datum: UpstreamDatum::GoImportPath(go_import_path.as_str().unwrap().to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.to_string_lossy().to_string()),
        };
        return vec![upstream_datum];
    }

    vec![]
}

/// Guess upstream metadata from a META.yml file.
///
/// See http://module-build.sourceforge.net/META-spec-v1.4.html for the
/// specification of the format.
pub fn guess_from_meta_yml(path: &Path, trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => {
            error!("Unable to open file: {}", path.display());
            return vec![];
        }
    };

    let mut contents = String::new();
    if file.read_to_string(&mut contents).is_err() {
        error!("Unable to read file: {}", path.display());
        return vec![];
    }

    let data: serde_yaml::Value = match serde_yaml::from_str(&contents) {
        Ok(d) => d,
        Err(e) => {
            error!("Unable to parse YAML: {}", e);
            return vec![];
        }
    };

    let mut upstream_data = Vec::new();

    if let Some(name) = data.get("name") {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(name.as_str().unwrap().to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.to_string_lossy().to_string()),
        });
    }

    if let Some(license) = data.get("license") {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::License(license.as_str().unwrap().to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.to_string_lossy().to_string()),
        });
    }

    if let Some(version) = data.get("version") {
        upstream_data.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Version(version.as_str().unwrap().to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.to_string_lossy().to_string()),
        });
    }

    if let Some(resources) = data.get("resources") {
        if let Some(bugtracker) = resources.get("bugtracker") {
            upstream_data.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::BugDatabase(bugtracker.as_str().unwrap().to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }

        if let Some(homepage) = resources.get("homepage") {
            upstream_data.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Homepage(homepage.as_str().unwrap().to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }

        if let Some(repository) = resources.get("repository") {
            if let Some(url) = repository.get("url") {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(url.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            } else {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(repository.as_str().unwrap().to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
        }
    }

    // Wild guess:
    if let Some(dist_name) = data.get("name") {
        upstream_data.extend(guess_from_perl_dist_name(path, dist_name.as_str().unwrap()));
    }

    upstream_data
}

pub fn metadata_from_itp_bug_body(body: &str) -> Vec<UpstreamDatumWithMetadata> {
    let mut results: Vec<UpstreamDatumWithMetadata> = Vec::new();
    // Skip first few lines with bug metadata (severity, owner, etc)
    let mut line_iter = body.split_terminator('\n');
    let mut next_line = line_iter.next();

    while let Some(line) = next_line {
        if next_line.is_none() {
            return vec![];
        }
        next_line = line_iter.next();
        if line.trim().is_empty() {
            break;
        }
    }

    while let Some(line) = next_line {
        if next_line.is_none() {
            return vec![];
        }
        if !line.is_empty() {
            break;
        }
        next_line = line_iter.next();
    }

    while let Some(mut line) = next_line {
        line = line.trim_start_matches('*').trim_start();

        if line.is_empty() {
            break;
        }

        match line.split_once(':') {
            Some((key, value)) => {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "Package name" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Name(value.to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: None,
                        });
                    }
                    "Version" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Version(value.to_string()),
                            certainty: Some(Certainty::Possible),
                            origin: None,
                        });
                    }
                    "Upstream Author" if !value.is_empty() => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Author(vec![Person::from(value)]),
                            certainty: Some(Certainty::Confident),
                            origin: None,
                        });
                    }
                    "URL" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Homepage(value.to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: None,
                        });
                    }
                    "License" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::License(value.to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: None,
                        });
                    }
                    "Description" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Summary(value.to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: None,
                        });
                    }
                    _ => {
                        debug!("Unknown pseudo-header {} in ITP bug body", key);
                    }
                }
            }
            _ => {
                debug!("Ignoring non-semi-field line {}", line);
            }
        }

        next_line = line_iter.next();
    }

    let mut rest: Vec<String> = Vec::new();
    for line in line_iter {
        if line.trim() == "-- System Information:" {
            break;
        }
        rest.push(line.to_string());
    }

    results.push(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Description(rest.join("\n")),
        certainty: Some(Certainty::Likely),
        origin: None,
    });

    results
}

// See https://www.freedesktop.org/software/appstream/docs/chap-Metadata.html
pub fn guess_from_metainfo(path: &Path, trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    let file = File::open(path).expect("Failed to open file");
    let root = Element::parse(file).expect("Failed to parse XML");

    let mut results: Vec<UpstreamDatumWithMetadata> = Vec::new();

    for child in root.children {
        let child = if let Some(element) = child.as_element() {
            element
        } else {
            continue;
        };
        if child.name == "id" {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(child.get_text().unwrap().to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
        if child.name == "project_license" {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::License(child.get_text().unwrap().to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
        if child.name == "url" {
            if let Some(urltype) = child.attributes.get("type") {
                if urltype == "homepage" {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Homepage(child.get_text().unwrap().to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                } else if urltype == "bugtracker" {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::BugDatabase(child.get_text().unwrap().to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
        }
        if child.name == "description" {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Description(child.get_text().unwrap().to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
        if child.name == "summary" {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Summary(child.get_text().unwrap().to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
        if child.name == "name" {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(child.get_text().unwrap().to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
    }

    results
}

// See https://github.com/ewilderj/doap
pub fn guess_from_doap(path: &Path, trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    let file = File::open(path).expect("Failed to open file");
    let doc = Element::parse(file).expect("Failed to parse XML");
    let mut root = &doc;

    let mut results: Vec<UpstreamDatumWithMetadata> = Vec::new();

    const DOAP_NAMESPACE: &str = "http://usefulinc.com/ns/doap#";
    const RDF_NAMESPACE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
    const SCHEMA_NAMESPACE: &str = "https://schema.org/";

    if root.name == "RDF" && root.namespace.as_deref() == Some(RDF_NAMESPACE) {
        for child in root.children.iter() {
            if let Some(element) = child.as_element() {
                root = element;
                break;
            }
        }
    }

    if root.name != "Project" || root.namespace.as_deref() != Some(DOAP_NAMESPACE) {
        error!(
            "Doap file does not have DOAP project as root, but {}",
            root.name
        );
        return results;
    }

    fn extract_url(el: &Element) -> Option<&str> {
        el.attributes.get("resource").map(|url| url.as_str())
    }

    fn extract_lang(el: &Element) -> Option<&str> {
        el.attributes.get("lang").map(|lang| lang.as_str())
    }

    let mut screenshots: Vec<String> = Vec::new();
    let mut maintainers: Vec<Person> = Vec::new();

    for child in &root.children {
        let child = if let Some(element) = child.as_element() {
            element
        } else {
            continue;
        };
        match (child.namespace.as_deref(), child.name.as_str()) {
            (Some(DOAP_NAMESPACE), "name") => {
                if let Some(text) = &child.get_text() {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Name(text.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            (Some(DOAP_NAMESPACE), "shortname") | (Some(DOAP_NAMESPACE), "short-name") => {
                if let Some(text) = &child.get_text() {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Name(text.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            (Some(DOAP_NAMESPACE), "bug-database") => {
                if let Some(url) = extract_url(child) {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::BugDatabase(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            (Some(DOAP_NAMESPACE), "homepage") => {
                if let Some(url) = extract_url(child) {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Homepage(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            (Some(DOAP_NAMESPACE), "download-page") => {
                if let Some(url) = extract_url(child) {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Download(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            (Some(DOAP_NAMESPACE), "shortdesc") => {
                if let Some(lang) = extract_lang(child) {
                    if lang == "en" {
                        if let Some(text) = &child.get_text() {
                            results.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::Summary(text.to_string()),
                                certainty: Some(Certainty::Certain),
                                origin: Some(path.to_string_lossy().to_string()),
                            });
                        }
                    }
                }
            }
            (Some(DOAP_NAMESPACE), "description") => {
                if let Some(lang) = extract_lang(child) {
                    if lang == "en" {
                        if let Some(text) = &child.get_text() {
                            results.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::Description(text.to_string()),
                                certainty: Some(Certainty::Certain),
                                origin: Some(path.to_string_lossy().to_string()),
                            });
                        }
                    }
                }
            }
            (Some(DOAP_NAMESPACE), "license") => {
                // TODO: Handle license
            }
            (Some(DOAP_NAMESPACE), "repository") => {
                for repo in &child.children {
                    let repo = if let Some(element) = repo.as_element() {
                        element
                    } else {
                        continue;
                    };
                    match repo.name.as_str() {
                        "SVNRepository" | "GitRepository" => {
                            if let Some(repo_location) = repo.get_child("location") {
                                if let Some(repo_url) = extract_url(repo_location) {
                                    results.push(UpstreamDatumWithMetadata {
                                        datum: UpstreamDatum::Repository(repo_url.to_string()),
                                        certainty: Some(Certainty::Certain),
                                        origin: Some(path.to_string_lossy().to_string()),
                                    });
                                }
                            }
                            if let Some(web_location) = repo.get_child("browse") {
                                if let Some(web_url) = extract_url(web_location) {
                                    results.push(UpstreamDatumWithMetadata {
                                        datum: UpstreamDatum::RepositoryBrowse(web_url.to_string()),
                                        certainty: Some(Certainty::Certain),
                                        origin: Some(path.to_string_lossy().to_string()),
                                    });
                                }
                            }
                        }
                        _ => (),
                    }
                }
            }
            (Some(DOAP_NAMESPACE), "category")
            | (Some(DOAP_NAMESPACE), "programming-language")
            | (Some(DOAP_NAMESPACE), "os")
            | (Some(DOAP_NAMESPACE), "implements")
            | (Some(SCHEMA_NAMESPACE), "logo")
            | (Some(DOAP_NAMESPACE), "platform") => {
                // TODO: Handle other tags
            }
            (Some(SCHEMA_NAMESPACE), "screenshot") => {
                if let Some(url) = extract_url(child) {
                    screenshots.push(url.to_string());
                }
            }
            (Some(DOAP_NAMESPACE), "wiki") => {
                if let Some(url) = extract_url(child) {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Wiki(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            (Some(DOAP_NAMESPACE), "maintainer") => {
                for person in &child.children {
                    let person = if let Some(element) = person.as_element() {
                        element
                    } else {
                        continue;
                    };
                    if person.name != "Person" {
                        continue;
                    }
                    let name = if let Some(name_tag) = person.get_child("name") {
                        name_tag.get_text().clone()
                    } else {
                        None
                    };
                    let email = if let Some(email_tag) = person.get_child("mbox") {
                        email_tag.get_text().as_ref().cloned()
                    } else {
                        None
                    };
                    let url = if let Some(email_tag) = person.get_child("mbox") {
                        extract_url(email_tag).map(|url| url.to_string())
                    } else {
                        None
                    };
                    maintainers.push(Person {
                        name: name.map(|n| n.to_string()),
                        email: email.map(|n| n.to_string()),
                        url,
                    });
                }
            }
            (Some(DOAP_NAMESPACE), "mailing-list") => {
                if let Some(url) = extract_url(child) {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::MailingList(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
            _ => {
                error!("Unknown tag {} in DOAP file", child.name);
            }
        }
    }

    if maintainers.len() == 1 {
        let maintainer = maintainers.remove(0);
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Maintainer(maintainer),
            certainty: Some(Certainty::Certain),
            origin: Some(path.to_string_lossy().to_string()),
        });
    } else {
        for maintainer in maintainers {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Maintainer(maintainer),
                certainty: Some(Certainty::Possible),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
    }

    results
}

// Documentation: https://opam.ocaml.org/doc/Manual.html#Package-definitions
pub fn guess_from_opam(path: &Path, trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    use opam_file_rs::value::{OpamFileItem, OpamFileSection, ValueKind};
    let mut f = File::open(path).unwrap();
    let mut contents = String::new();
    f.read_to_string(&mut contents).unwrap();
    let opam = opam_file_rs::parse(contents.as_str()).unwrap();
    let mut results: Vec<UpstreamDatumWithMetadata> = Vec::new();

    fn find_item<'a>(section: &'a OpamFileSection, name: &str) -> Option<&'a OpamFileItem> {
        for child in section.section_item.iter() {
            match child {
                OpamFileItem::Variable(_, n, _) if n == name => return Some(child),
                _ => (),
            }
        }
        None
    }

    for entry in opam.file_contents {
        match entry {
            OpamFileItem::Variable(_, name, value) if name == "maintainer" => {
                let value = match value.kind {
                    ValueKind::String(s) => s,
                    _ => {
                        warn!("Unexpected type for maintainer in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Maintainer(Person::from(value.as_str())),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Variable(_, name, value) if name == "license" => {
                let value = match value.kind {
                    ValueKind::String(s) => s,
                    _ => {
                        warn!("Unexpected type for license in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License(value),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Variable(_, name, value) if name == "homepage" => {
                let value = match value.kind {
                    ValueKind::String(s) => s,
                    _ => {
                        warn!("Unexpected type for homepage in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Homepage(value),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Section(_, section)
                if section.section_name.as_deref() == Some("dev-repo") =>
            {
                let value = find_item(&section, "repository").unwrap();
                match value {
                    OpamFileItem::Variable(_, _, ref value) => {
                        let value = match value.kind {
                            ValueKind::String(ref s) => s,
                            _ => {
                                warn!("Unexpected type for dev-repo in OPAM file: {:?}", value);
                                continue;
                            }
                        };
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Repository(value.to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: Some(path.to_string_lossy().to_string()),
                        });
                    }
                    _ => {
                        warn!("Unexpected type for dev-repo in OPAM file: {:?}", value);
                        continue;
                    }
                }
            }
            OpamFileItem::Variable(_, name, value) if name == "bug-reports" => {
                let value = match value.kind {
                    ValueKind::String(s) => s,
                    _ => {
                        warn!("Unexpected type for bug-reports in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::BugDatabase(value),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Variable(_, name, value) if name == "synopsis" => {
                let value = match value.kind {
                    ValueKind::String(s) => s,
                    _ => {
                        warn!("Unexpected type for synopsis in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(value),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Variable(_, name, value) if name == "description" => {
                let value = match value.kind {
                    ValueKind::String(s) => s,
                    _ => {
                        warn!("Unexpected type for description in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Description(value),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Variable(_, name, value) if name == "doc" => {
                let value = match value.kind {
                    ValueKind::String(s) => s,
                    _ => {
                        warn!("Unexpected type for doc in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Documentation(value),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Variable(_, name, value) if name == "version" => {
                let value = match value.kind {
                    ValueKind::String(s) => s,
                    _ => {
                        warn!("Unexpected type for version in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(value),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Variable(_, name, value) if name == "authors" => {
                let value = match value.kind {
                    ValueKind::String(s) => vec![Person::from(s.as_str())],
                    ValueKind::List(ref l) => l
                        .iter()
                        .filter_map(|v| match v.kind {
                            ValueKind::String(ref s) => Some(Person::from(s.as_str())),
                            _ => {
                                warn!("Unexpected type for authors in OPAM file: {:?}", &value);
                                None
                            }
                        })
                        .collect(),
                    _ => {
                        warn!("Unexpected type for authors in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Author(value),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Variable(_, name, _) => {
                warn!("Unexpected variable in OPAM file: {}", name);
            }
            OpamFileItem::Section(_, section) => {
                warn!("Unexpected section in OPAM file: {:?}", section);
            }
        }
    }

    results
}

pub fn guess_from_pom_xml(path: &Path, trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    let file = File::open(path).expect("Failed to open file");

    let root = match Element::parse(file) {
        Ok(root) => root,
        Err(e) => {
            error!("Unable to parse package.xml: {}", e);
            return vec![];
        }
    };

    let mut result = Vec::new();
    if root.name == "project" {
        if let Some(name_tag) = root.get_child("name") {
            if let Some(name) = name_tag.get_text() {
                if !name.contains('$') {
                    result.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Name(name.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
        } else if let Some(artifact_id_tag) = root.get_child("artifactId") {
            if let Some(artifact_id) = artifact_id_tag.get_text() {
                result.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(artifact_id.to_string()),
                    certainty: Some(Certainty::Possible),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
        }

        if let Some(description_tag) = root.get_child("description") {
            if let Some(description) = description_tag.get_text() {
                result.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(description.to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
        }

        if let Some(version_tag) = root.get_child("version") {
            if let Some(version) = version_tag.get_text() {
                if !version.contains('$') {
                    result.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Version(version.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
        }

        if let Some(licenses_tag) = root.get_child("licenses") {
            for license_tag in licenses_tag
                .children
                .iter()
                .filter(|c| c.as_element().map_or(false, |e| e.name == "license"))
            {
                let license_tag = license_tag.as_element().unwrap();
                if let Some(name_tag) = license_tag.get_child("name") {
                    if let Some(license_name) = name_tag.get_text() {
                        result.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::License(license_name.to_string()),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.to_string_lossy().to_string()),
                        });
                    }
                }
            }
        }

        for scm_tag in root
            .children
            .iter()
            .filter(|c| c.as_element().map_or(false, |e| e.name == "scm"))
        {
            let scm_tag = scm_tag.as_element().unwrap();
            if let Some(url_tag) = scm_tag.get_child("url") {
                if let Some(url) = url_tag.get_text() {
                    if url.starts_with("scm:") && url.matches(':').count() >= 3 {
                        let url_parts: Vec<&str> = url.splitn(3, ':').collect();
                        let browse_url = url_parts[2];
                        if vcs::plausible_browse_url(browse_url) {
                            result.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::RepositoryBrowse(browse_url.to_owned()),
                                certainty: Some(Certainty::Certain),
                                origin: Some(path.to_string_lossy().to_string()),
                            });
                        }
                    } else {
                        result.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::RepositoryBrowse(url.to_string()),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.to_string_lossy().to_string()),
                        });
                    }
                }
            }

            if let Some(connection_tag) = scm_tag.get_child("connection") {
                if let Some(connection) = connection_tag.get_text() {
                    let connection_parts: Vec<&str> = connection.splitn(3, ':').collect();
                    if connection_parts.len() == 3 && connection_parts[0] == "scm" {
                        result.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Repository(connection_parts[2].to_owned()),
                            certainty: Some(Certainty::Certain),
                            origin: Some(path.to_string_lossy().to_string()),
                        });
                    } else {
                        warn!("Invalid format for SCM connection: {}", connection);
                    }
                }
            }
        }

        for issue_mgmt_tag in root.children.iter().filter(|c| {
            c.as_element()
                .map_or(false, |e| e.name == "issueManagement")
        }) {
            let issue_mgmt_tag = issue_mgmt_tag.as_element().unwrap();
            if let Some(url_tag) = issue_mgmt_tag.get_child("url") {
                if let Some(url) = url_tag.get_text() {
                    result.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::BugDatabase(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
        }

        if let Some(url_tag) = root.get_child("url") {
            if let Some(url) = url_tag.get_text() {
                if !url.starts_with("scm:") {
                    result.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Homepage(url.into_owned()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                }
            }
        }
    }

    result
}

pub fn guess_from_wscript(path: &Path, trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    let file = File::open(path).expect("Failed to open file");
    let reader = BufReader::new(file);
    let mut results = Vec::new();
    let appname_regex = Regex::new("APPNAME = [\'\"](.*)[\'\"]").unwrap();
    let version_regex = Regex::new("VERSION = [\'\"](.*)[\'\"]").unwrap();

    for line in reader.lines() {
        if let Ok(line) = line {
            if let Some(captures) = appname_regex.captures(&line) {
                let name = captures.get(1).unwrap().as_str().to_owned();
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(name),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            if let Some(captures) = version_regex.captures(&line) {
                let version = captures.get(1).unwrap().as_str().to_owned();
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(version),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
        }
    }

    results
}

pub fn guess_from_makefile_pl(path: &Path, trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
    let mut dist_name = None;
    let file = File::open(path).expect("Failed to open file");
    let reader = BufReader::new(file);
    let mut results = Vec::new();
    let name_regex = Regex::new("name '([^'\"]+)';$").unwrap();
    let repository_regex = Regex::new("repository '([^'\"]+)';$").unwrap();

    for line in reader.lines() {
        if let Ok(line) = line {
            if let Some(captures) = name_regex.captures(&line) {
                dist_name = Some(captures.get(1).unwrap().as_str().to_owned());
                let name = dist_name.as_ref().unwrap().to_owned();
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(name),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            if let Some(captures) = repository_regex.captures(&line) {
                let repository = captures.get(1).unwrap().as_str().to_owned();
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(repository),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
        }
    }

    if let Some(dist_name) = dist_name {
        results.extend(guess_from_perl_dist_name(path, &dist_name));
    }

    results
}

// See https://golang.org/doc/modules/gomod-ref
pub fn guess_from_go_mod(path: &Path, trust_package: bool) -> Vec<UpstreamDatumWithMetadata> {
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
