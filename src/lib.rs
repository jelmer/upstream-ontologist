use lazy_regex::regex;
use log::{debug, error, warn};
use percent_encoding::utf8_percent_encode;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use reqwest::header::HeaderMap;
use std::str::FromStr;

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use url::Url;

static USER_AGENT: &str = concat!("upstream-ontologist/", env!("CARGO_PKG_VERSION"));

// Too aggressive?
const DEFAULT_URLLIB_TIMEOUT: u64 = 3;

pub mod providers;
pub mod readme;
pub mod vcs;

#[derive(Clone, Copy, Debug, Ord, Eq, PartialOrd, PartialEq)]
pub enum Certainty {
    Certain,
    Confident,
    Likely,
    Possible,
}

impl FromStr for Certainty {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "certain" => Ok(Certainty::Certain),
            "confident" => Ok(Certainty::Confident),
            "likely" => Ok(Certainty::Likely),
            "possible" => Ok(Certainty::Possible),
            _ => Err(format!("unknown certainty: {}", s)),
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

#[derive(Default, Clone, Debug)]
pub struct Person {
    pub name: Option<String>,
    pub email: Option<String>,
    pub url: Option<String>,
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
            name: Some(text),
            ..Default::default()
        }
    }
}

fn parseaddr(text: &str) -> Option<(String, String)> {
    let re = regex!(r"(.*?)\s*<([^<>]+)>");
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

#[derive(Clone, Debug)]
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
    SourceForgeProject(String),
    Archive(String),
    Demo(String),
    PeclPackage(String),
    Funding(String),
    Changelog(String),
}

#[derive(Clone)]
pub struct UpstreamDatumWithMetadata {
    pub datum: UpstreamDatum,
    pub origin: Option<String>,
    pub certainty: Option<Certainty>,
}

fn known_bad_url(value: &str) -> bool {
    if value.contains("${") {
        return true;
    }
    false
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
            UpstreamDatum::SourceForgeProject(..) => "SourceForge-Project",
            UpstreamDatum::Archive(..) => "Archive",
            UpstreamDatum::Demo(..) => "Demo",
            UpstreamDatum::PeclPackage(..) => "Pecl-Package",
            UpstreamDatum::Funding(..) => "Funding",
            UpstreamDatum::Changelog(..) => "Changelog",
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            UpstreamDatum::Name(s) => Some(s),
            UpstreamDatum::Homepage(s) => Some(s),
            UpstreamDatum::Repository(s) => Some(s),
            UpstreamDatum::RepositoryBrowse(s) => Some(s),
            UpstreamDatum::Description(s) => Some(s),
            UpstreamDatum::Summary(s) => Some(s),
            UpstreamDatum::License(s) => Some(s),
            UpstreamDatum::BugDatabase(s) => Some(s),
            UpstreamDatum::BugSubmit(s) => Some(s),
            UpstreamDatum::Contact(s) => Some(s),
            UpstreamDatum::CargoCrate(s) => Some(s),
            UpstreamDatum::SecurityMD(s) => Some(s),
            UpstreamDatum::SecurityContact(s) => Some(s),
            UpstreamDatum::Version(s) => Some(s),
            UpstreamDatum::Documentation(s) => Some(s),
            UpstreamDatum::GoImportPath(s) => Some(s),
            UpstreamDatum::Download(s) => Some(s),
            UpstreamDatum::Wiki(s) => Some(s),
            UpstreamDatum::MailingList(s) => Some(s),
            UpstreamDatum::SourceForgeProject(s) => Some(s),
            UpstreamDatum::Archive(s) => Some(s),
            UpstreamDatum::Demo(s) => Some(s),
            UpstreamDatum::PeclPackage(s) => Some(s),
            UpstreamDatum::Author(..) => None,
            UpstreamDatum::Maintainer(..) => None,
            UpstreamDatum::Keywords(..) => None,
            UpstreamDatum::Copyright(c) => Some(c),
            UpstreamDatum::Funding(f) => Some(f),
            UpstreamDatum::Changelog(c) => Some(c),
        }
    }

    pub fn known_bad_guess(&self) -> bool {
        match self {
            UpstreamDatum::BugDatabase(s) | UpstreamDatum::BugSubmit(s) => {
                if known_bad_url(s) {
                    return true;
                }
                let url = match Url::parse(s) {
                    Ok(url) => url,
                    Err(_) => return false,
                };
                if url.host_str() == Some("bugzilla.gnome.org") {
                    return true;
                }
                if url.host_str() == Some("bugs.freedesktop.org") {
                    return true;
                }
                if url.path().ends_with("/sign_in") {
                    return true;
                }
            }
            UpstreamDatum::Repository(s) => {
                if known_bad_url(s) {
                    return true;
                }
                let url = match Url::parse(s) {
                    Ok(url) => url,
                    Err(_) => return false,
                };
                if url.host_str() == Some("anongit.kde.org") {
                    return true;
                }
                if url.host_str() == Some("git.gitorious.org") {
                    return true;
                }
                if url.path().ends_with("/sign_in") {
                    return true;
                }
            }
            UpstreamDatum::Homepage(s) => {
                let url = match Url::parse(s) {
                    Ok(url) => url,
                    Err(_) => return false,
                };

                if url.host_str() == Some("pypi.org") {
                    return true;
                }
                if url.host_str() == Some("rubygems.org") {
                    return true;
                }
            }
            UpstreamDatum::RepositoryBrowse(s) => {
                if known_bad_url(s) {
                    return true;
                }
                let url = match Url::parse(s) {
                    Ok(url) => url,
                    Err(_) => return false,
                };
                if url.host_str() == Some("cgit.kde.org") {
                    return true;
                }
                if url.path().ends_with("/sign_in") {
                    return true;
                }
            }
            UpstreamDatum::Author(authors) => {
                for a in authors {
                    if let Some(name) = &a.name {
                        let lc = name.to_lowercase();
                        if lc.contains("unknown") {
                            return true;
                        }
                        if lc.contains("maintainer") {
                            return true;
                        }
                        if lc.contains("contributor") {
                            return true;
                        }
                    }
                }
            }
            UpstreamDatum::Name(s) => {
                let lc = s.to_lowercase();
                if lc.contains("unknown") {
                    return true;
                }
                if lc == "package" {
                    return true;
                }
            }
            UpstreamDatum::Version(s) => {
                let lc = s.to_lowercase();
                if ["devel", "unknown"].contains(&lc.as_str()) {
                    return true;
                }
            }
            _ => {}
        }
        false
    }
}

pub struct UpstreamMetadata(Vec<UpstreamDatumWithMetadata>);

impl UpstreamMetadata {
    pub fn new() -> Self {
        UpstreamMetadata(Vec::new())
    }

    pub fn iter(&self) -> impl Iterator<Item = &UpstreamDatumWithMetadata> {
        self.0.iter()
    }

    pub fn get_field(&self, field: &str) -> Option<&UpstreamDatumWithMetadata> {
        self.0.iter().find(|d| d.datum.field() == field)
    }

    pub fn has_field(&self, field: &str) -> bool {
        self.get_field(field).is_some()
    }

    pub fn discard_known_bad(&mut self) {
        self.0.retain(|d| !d.datum.known_bad_guess());
    }
}

impl From<Vec<UpstreamDatumWithMetadata>> for UpstreamMetadata {
    fn from(v: Vec<UpstreamDatumWithMetadata>) -> Self {
        UpstreamMetadata(v)
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
                "Security-MD" => UpstreamDatum::SecurityMD(value.extract::<String>(py).unwrap()),
                "Security-Contact" => {
                    UpstreamDatum::SecurityContact(value.extract::<String>(py).unwrap())
                }
                "Cargo-Crate" => UpstreamDatum::CargoCrate(value.extract::<String>(py).unwrap()),
                "Description" => UpstreamDatum::Description(value.extract::<String>(py).unwrap()),
                "Summary" => UpstreamDatum::Summary(value.extract::<String>(py).unwrap()),
                "License" => UpstreamDatum::License(value.extract::<String>(py).unwrap()),
                "Version" => UpstreamDatum::Version(value.extract::<String>(py).unwrap()),
                "Demo" => UpstreamDatum::Demo(value.extract::<String>(py).unwrap()),
                "Archive" => UpstreamDatum::Archive(value.extract::<String>(py).unwrap()),
                "Pecl-Package" => UpstreamDatum::PeclPackage(value.extract::<String>(py).unwrap()),
                "Author" => UpstreamDatum::Author(
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

fn xmlparse_simplify_namespaces(path: &Path, namespaces: &[&str]) -> Option<xmltree::Element> {
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

fn simplify_namespaces(element: &mut xmltree::Element, namespaces: &[String]) {
    use xmltree::XMLNode;
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

pub fn guess_from_metadata_json(
    path: &Path,
    _trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let data: serde_json::Map<String, serde_json::Value> = match serde_json::from_str(&contents) {
        Ok(data) => data,
        Err(e) => {
            return Err(ProviderError::ParseError(e.to_string()));
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
                    let authors: Vec<Person> = match author_values
                        .iter()
                        .map(|v| {
                            Ok::<Person, &str>(Person::from(
                                v.as_str().ok_or("Author value is not a string")?,
                            ))
                        })
                        .collect::<std::result::Result<Vec<_>, _>>()
                    {
                        Ok(authors) => authors,
                        Err(e) => {
                            warn!("Error parsing author array: {}", e);
                            continue;
                        }
                    };
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

    Ok(upstream_data)
}

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
    headers.insert(
        reqwest::header::USER_AGENT,
        USER_AGENT.parse().expect("valid user agent"),
    );
    let client = reqwest::blocking::Client::builder()
        .default_headers(headers)
        .timeout(timeout)
        .build()
        .map_err(|e| CanonicalizeError::Unverifiable(url.clone(), format!("HTTP error {}", e)))?;

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

pub fn with_path_segments(url: &Url, path_segments: &[&str]) -> Result<Url, ()> {
    let mut url = url.clone();
    url.path_segments_mut()?
        .clear()
        .extend(path_segments.iter());
    Ok(url)
}

pub trait Forge: Send + Sync {
    fn repository_browse_can_be_homepage(&self) -> bool;

    fn name(&self) -> &'static str;

    fn bug_database_url_from_bug_submit_url(&self, url: &Url) -> Option<Url> {
        None
    }

    fn bug_submit_url_from_bug_database_url(&self, url: &Url) -> Option<Url> {
        None
    }

    fn check_bug_database_canonical(&self, url: &Url) -> Result<Url, CanonicalizeError> {
        Err(CanonicalizeError::Unverifiable(
            url.clone(),
            "Not implemented".to_string(),
        ))
    }

    fn check_bug_submit_url_canonical(&self, url: &Url) -> Result<Url, CanonicalizeError> {
        Err(CanonicalizeError::Unverifiable(
            url.clone(),
            "Not implemented".to_string(),
        ))
    }

    fn bug_database_from_issue_url(&self, url: &Url) -> Option<Url> {
        None
    }

    fn bug_database_url_from_repo_url(&self, url: &Url) -> Option<Url> {
        None
    }

    fn repo_url_from_merge_request_url(&self, url: &Url) -> Option<Url> {
        None
    }

    fn extend_metadata(
        &self,
        metadata: &mut Vec<UpstreamDatumWithMetadata>,
        project: &str,
        max_certainty: Certainty,
    ) {
    }
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

        url.set_scheme("https").expect("valid scheme");

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
        url.set_scheme("https").expect("valid scheme");
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
        let data = response.json::<serde_json::Value>().map_err(|e| {
            CanonicalizeError::Unverifiable(
                url.clone(),
                format!("Unable to verify bug database URL: {}", e),
            )
        })?;

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

        let mut url = Url::parse(data["html_url"].as_str().ok_or_else(|| {
            CanonicalizeError::Unverifiable(
                url.clone(),
                "Unable to verify bug database URL: no html_url".to_string(),
            )
        })?)
        .map_err(|e| {
            CanonicalizeError::Unverifiable(
                url.clone(),
                format!("Unable to verify bug database URL: {}", e),
            )
        })?;

        url.set_scheme("https").expect("valid scheme");
        url.path_segments_mut()
            .expect("path segments")
            .push("issues");

        Ok(url)
    }

    fn check_bug_submit_url_canonical(&self, url: &Url) -> Result<Url, CanonicalizeError> {
        let mut path_segments = url.path_segments().unwrap().collect::<Vec<_>>();
        path_segments.pop();
        let db_url = with_path_segments(url, &path_segments).unwrap();
        let mut canonical_db_url = self.check_bug_database_canonical(&db_url)?;
        canonical_db_url.set_scheme("https").expect("valid scheme");
        canonical_db_url
            .path_segments_mut()
            .expect("path segments")
            .push("new");
        Ok(canonical_db_url)
    }

    fn bug_database_from_issue_url(&self, url: &Url) -> Option<Url> {
        let path_elements = url
            .path_segments()
            .expect("path segments")
            .collect::<Vec<_>>();
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
        let path_elements = url
            .path_segments()
            .expect("path segments")
            .collect::<Vec<_>>();
        if path_elements.len() < 2 || path_elements[1] != "issues" {
            return None;
        }
        let mut url = url.clone();
        url.set_scheme("https").expect("valid scheme");
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
        let mut path_elements = url
            .path_segments()
            .expect("path segments")
            .collect::<Vec<_>>();

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
        let path_elements = url
            .path_segments()
            .expect("path segments")
            .collect::<Vec<_>>();

        if path_elements.len() < 2 {
            return None;
        }
        if path_elements[path_elements.len() - 1] != "issues" {
            return None;
        }

        let mut url = url.clone();
        url.path_segments_mut().expect("path segments").push("new");

        Some(url)
    }

    fn check_bug_database_canonical(&self, url: &Url) -> Result<Url, CanonicalizeError> {
        let host = url
            .host()
            .ok_or_else(|| CanonicalizeError::InvalidUrl(url.clone(), "no host".to_string()))?;
        let mut path_elements = url
            .path_segments()
            .expect("path segments")
            .collect::<Vec<_>>();
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
        .map_err(|_| {
            CanonicalizeError::InvalidUrl(
                url.clone(),
                "GitLab URL with invalid project path".to_string(),
            )
        })?;
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
        let path_elements = url
            .path_segments()
            .expect("valid segments")
            .collect::<Vec<_>>();
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
        canonical_db_url
            .path_segments_mut()
            .expect("valid segments")
            .push("new");
        Ok(canonical_db_url)
    }

    fn bug_database_from_issue_url(&self, url: &Url) -> Option<Url> {
        let path_elements = url
            .path_segments()
            .expect("valid segments")
            .collect::<Vec<_>>();
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
        let last = url
            .path_segments()
            .expect("valid segments")
            .last()
            .unwrap()
            .to_string();
        url.path_segments_mut()
            .unwrap()
            .pop()
            .push(last.trim_end_matches(".git"))
            .push("issues");
        Some(url)
    }

    fn repo_url_from_merge_request_url(&self, url: &Url) -> Option<Url> {
        let path_elements = url
            .path_segments()
            .expect("path segments")
            .collect::<Vec<_>>();
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

pub fn guess_from_travis_yml(
    path: &Path,
    _trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut file = File::open(path)?;

    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let data: serde_yaml::Value =
        serde_yaml::from_str(&contents).map_err(|e| ProviderError::ParseError(e.to_string()))?;

    let mut ret = Vec::new();

    if let Some(go_import_path) = data.get("go_import_path") {
        if let Some(go_import_path) = go_import_path.as_str() {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::GoImportPath(go_import_path.to_string()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
    }

    Ok(ret)
}

pub fn guess_from_environment() -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError>
{
    let mut results = Vec::new();
    if let Ok(url) = std::env::var("UPSTREAM_BRANCH_URL") {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository(url),
            certainty: Some(Certainty::Certain),
            origin: Some("environment".to_string()),
        });
    }
    Ok(results)
}

// Documentation: https://docs.microsoft.com/en-us/nuget/reference/nuspec
pub fn guess_from_nuspec(
    path: &Path,
    trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    const NAMESPACES: &[&str] = &["http://schemas.microsoft.com/packaging/2010/07/nuspec.xsd"];
    // XML parsing and other logic
    let root = match xmlparse_simplify_namespaces(path, NAMESPACES) {
        Some(root) => root,
        None => {
            return Err(ProviderError::ParseError(
                "Unable to parse nuspec".to_string(),
            ));
        }
    };

    assert_eq!(root.name, "package", "root tag is {}", root.name);
    let metadata = root.get_child("metadata");
    if metadata.is_none() {
        return Err(ProviderError::ParseError(
            "Unable to find metadata tag".to_string(),
        ));
    }
    let metadata = metadata.unwrap();

    let mut result = Vec::new();

    if let Some(version_tag) = metadata.get_child("version") {
        if let Some(version) = version_tag.get_text() {
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Version(version.into_owned()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
    }

    if let Some(description_tag) = metadata.get_child("description") {
        if let Some(description) = description_tag.get_text() {
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Description(description.into_owned()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
    }

    if let Some(authors_tag) = metadata.get_child("authors") {
        if let Some(authors) = authors_tag.get_text() {
            let authors = authors.split(',').map(Person::from).collect();
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Author(authors),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
    }

    if let Some(project_url_tag) = metadata.get_child("projectUrl") {
        if let Some(project_url) = project_url_tag.get_text() {
            let repo_url = vcs::guess_repo_from_url(&url::Url::parse(&project_url).unwrap(), None);
            if let Some(repo_url) = repo_url {
                result.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(repo_url),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Homepage(project_url.into_owned()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
    }

    if let Some(license_tag) = metadata.get_child("license") {
        if let Some(license) = license_tag.get_text() {
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::License(license.into_owned()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
    }

    if let Some(copyright_tag) = metadata.get_child("copyright") {
        if let Some(copyright) = copyright_tag.get_text() {
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Copyright(copyright.into_owned()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
    }

    if let Some(title_tag) = metadata.get_child("title") {
        if let Some(title) = title_tag.get_text() {
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(title.into_owned()),
                certainty: Some(Certainty::Likely),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
    }

    if let Some(summary_tag) = metadata.get_child("summary") {
        if let Some(summary) = summary_tag.get_text() {
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Summary(summary.into_owned()),
                certainty: Some(Certainty::Likely),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
    }

    if let Some(repository_tag) = metadata.get_child("repository") {
        if let Some(repo_url) = repository_tag.attributes.get("url") {
            let branch = repository_tag.attributes.get("branch");
            result.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(vcs::unsplit_vcs_url(
                    repo_url,
                    branch.map(|s| s.as_str()),
                    None,
                )),
                certainty: Some(Certainty::Certain),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
    }

    Ok(result)
}

fn find_datum<'a>(
    metadata: &'a [UpstreamDatumWithMetadata],
    field: &str,
) -> Option<&'a UpstreamDatumWithMetadata> {
    metadata.iter().find(|d| d.datum.field() == field)
}

fn set_datum(metadata: &mut Vec<UpstreamDatumWithMetadata>, datum: UpstreamDatumWithMetadata) {
    if let Some(idx) = metadata
        .iter()
        .position(|d| d.datum.field() == datum.datum.field())
    {
        metadata[idx] = datum;
    } else {
        metadata.push(datum);
    }
}

fn update_from_guesses(
    metadata: &mut Vec<UpstreamDatumWithMetadata>,
    new_items: Vec<UpstreamDatumWithMetadata>,
) -> Vec<UpstreamDatumWithMetadata> {
    let mut changed = vec![];
    for datum in new_items {
        let current_datum = find_datum(metadata, datum.datum.field());
        if current_datum.is_none() || datum.certainty < current_datum.unwrap().certainty {
            changed.push(datum.clone());
            set_datum(metadata, datum);
        }
    }
    changed
}

fn possible_fields_missing(
    upstream_metadata: &[UpstreamDatumWithMetadata],
    fields: &[&str],
    field_certainty: Certainty,
) -> bool {
    for field in fields {
        match find_datum(upstream_metadata, field) {
            Some(datum) if datum.certainty != Some(Certainty::Certain) => return true,
            None => return true,
            _ => (),
        }
    }
    false
}

fn extend_from_external_guesser(
    metadata: &mut Vec<UpstreamDatumWithMetadata>,
    max_certainty: Certainty,
    supported_fields: &[&str],
    new_items: impl Fn() -> Vec<UpstreamDatum>,
) {
    if !possible_fields_missing(metadata, supported_fields, max_certainty) {
        return;
    }

    let new_items = new_items()
        .into_iter()
        .map(|item| UpstreamDatumWithMetadata {
            datum: item,
            certainty: Some(max_certainty),
            origin: None,
        })
        .collect();

    update_from_guesses(metadata, new_items);
}

pub struct SourceForge;

impl SourceForge {
    pub fn new() -> Self {
        Self
    }
}

impl Forge for SourceForge {
    fn name(&self) -> &'static str {
        "SourceForge"
    }
    fn repository_browse_can_be_homepage(&self) -> bool {
        false
    }

    fn bug_database_url_from_bug_submit_url(&self, url: &Url) -> Option<Url> {
        let mut segments = url.path_segments()?;
        if segments.next() != Some("p") {
            return None;
        }
        let project = segments.next()?;
        if segments.next() != Some("bugs") {
            return None;
        }
        with_path_segments(url, &["p", project, "bugs"]).ok()
    }

    fn extend_metadata(
        &self,
        metadata: &mut Vec<UpstreamDatumWithMetadata>,
        project: &str,
        max_certainty: Certainty,
    ) {
        let subproject = find_datum(metadata, "Name").map_or(None, |f| match f.datum {
            UpstreamDatum::Name(ref name) => Some(name.to_string()),
            _ => None,
        });

        extend_from_external_guesser(
            metadata,
            max_certainty,
            &["Homepage", "Name", "Repository", "Bug-Database"],
            || guess_from_sf(project, subproject.as_deref()),
        )
    }
}

pub struct Launchpad;

impl Launchpad {
    pub fn new() -> Self {
        Self
    }
}

impl Forge for Launchpad {
    fn name(&self) -> &'static str {
        "launchpad"
    }

    fn repository_browse_can_be_homepage(&self) -> bool {
        false
    }
    fn bug_database_url_from_bug_submit_url(&self, url: &Url) -> Option<Url> {
        if url.host_str()? != "bugs.launchpad.net" {
            return None;
        }

        let mut segments = url.path_segments()?;
        let project = segments.next()?;

        with_path_segments(url, &[project]).ok()
    }

    fn bug_submit_url_from_bug_database_url(&self, url: &Url) -> Option<Url> {
        if url.host_str()? != "bugs.launchpad.net" {
            return None;
        }

        let mut segments = url.path_segments()?;
        let project = segments.next()?;

        with_path_segments(url, &[project, "+filebug"]).ok()
    }
}

pub fn find_forge(url: &Url, net_access: Option<bool>) -> Option<Box<dyn Forge>> {
    if url.host_str()? == "sourceforge.net" {
        return Some(Box::new(SourceForge::new()));
    }

    if url.host_str()?.ends_with(".launchpad.net") {
        return Some(Box::new(Launchpad::new()));
    }

    if url.host_str()? == "github.com" {
        return Some(Box::new(GitHub::new()));
    }

    if vcs::is_gitlab_site(url.host_str()?, net_access) {
        return Some(Box::new(GitLab::new()));
    }

    None
}

pub fn check_bug_database_canonical(
    url: &Url,
    net_access: Option<bool>,
) -> Result<Url, CanonicalizeError> {
    if let Some(forge) = find_forge(url, net_access) {
        forge
            .bug_database_url_from_bug_submit_url(url)
            .ok_or(CanonicalizeError::Unverifiable(
                url.clone(),
                "no bug database URL found".to_string(),
            ))
    } else {
        Err(CanonicalizeError::Unverifiable(
            url.clone(),
            "unknown forge".to_string(),
        ))
    }
}

pub fn bug_submit_url_from_bug_database_url(url: &Url, net_access: Option<bool>) -> Option<Url> {
    if let Some(forge) = find_forge(url, net_access) {
        forge.bug_submit_url_from_bug_database_url(url)
    } else {
        None
    }
}

pub fn bug_database_url_from_bug_submit_url(url: &Url, net_access: Option<bool>) -> Option<Url> {
    if let Some(forge) = find_forge(url, net_access) {
        forge.bug_database_url_from_bug_submit_url(url)
    } else {
        None
    }
}

pub fn guess_bug_database_url_from_repo_url(url: &Url, net_access: Option<bool>) -> Option<Url> {
    if let Some(forge) = find_forge(url, net_access) {
        forge.bug_database_url_from_repo_url(url)
    } else {
        None
    }
}

pub fn repo_url_from_merge_request_url(url: &Url, net_access: Option<bool>) -> Option<Url> {
    if let Some(forge) = find_forge(url, net_access) {
        forge.repo_url_from_merge_request_url(url)
    } else {
        None
    }
}

pub fn bug_database_from_issue_url(url: &Url, net_access: Option<bool>) -> Option<Url> {
    if let Some(forge) = find_forge(url, net_access) {
        forge.bug_database_from_issue_url(url)
    } else {
        None
    }
}

pub fn check_bug_submit_url_canonical(
    url: &Url,
    net_access: Option<bool>,
) -> Result<Url, CanonicalizeError> {
    if let Some(forge) = find_forge(url, net_access) {
        forge
            .bug_submit_url_from_bug_database_url(url)
            .ok_or(CanonicalizeError::Unverifiable(
                url.clone(),
                "no bug submit URL found".to_string(),
            ))
    } else {
        Err(CanonicalizeError::Unverifiable(
            url.clone(),
            "unknown forge".to_string(),
        ))
    }
}

fn sf_git_extract_url(page: &str) -> Option<String> {
    use select::document::Document;
    use select::predicate::Attr;

    let soup = Document::from(page);

    let el = soup.find(Attr("id", "access_url")).next();
    el?;

    let el = el.unwrap();
    el.attr("value")?;

    let value = el.attr("value").unwrap();
    let access_command: Vec<&str> = value.split(' ').collect();
    if access_command.len() < 3 || access_command[..2] != ["git", "clone"] {
        return None;
    }

    Some(access_command[2].to_string())
}

pub fn get_sf_metadata(project: &str) -> Option<serde_json::Value> {
    let url = format!("https://sourceforge.net/rest/p/{}", project);
    match load_json_url(&Url::parse(url.as_str()).unwrap(), None) {
        Ok(data) => Some(data),
        Err(HTTPJSONError::Error { status, .. }) if status == reqwest::StatusCode::NOT_FOUND => {
            None
        }
        r => panic!("Unexpected result from {}: {:?}", url, r),
    }
}

pub fn guess_from_sf(sf_project: &str, subproject: Option<&str>) -> Vec<UpstreamDatum> {
    let mut results = Vec::new();
    match get_sf_metadata(sf_project) {
        Some(data) => {
            if let Some(name) = data.get("name") {
                results.push(UpstreamDatum::Name(name.to_string()));
            }
            if let Some(external_homepage) = data.get("external_homepage") {
                results.push(UpstreamDatum::Homepage(external_homepage.to_string()));
            }
            if let Some(preferred_support_url) = data.get("preferred_support_url") {
                let preferred_support_url = Url::parse(preferred_support_url.as_str().unwrap())
                    .expect("preferred_support_url is not a valid URL");
                match check_bug_database_canonical(&preferred_support_url, Some(true)) {
                    Ok(canonical_url) => {
                        results.push(UpstreamDatum::BugDatabase(canonical_url.to_string()));
                    }
                    Err(_) => {
                        results.push(UpstreamDatum::BugDatabase(
                            preferred_support_url.to_string(),
                        ));
                    }
                }
            }

            let vcs_names = ["hg", "git", "svn", "cvs", "bzr"];
            let mut vcs_tools: Vec<(&str, Option<&str>, &str)> =
                data.get("tools").map_or_else(Vec::new, |tools| {
                    tools
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|tool| {
                            vcs_names.contains(&tool.get("name").unwrap().as_str().unwrap())
                        })
                        .map(|tool| {
                            (
                                tool.get("name").map_or("", |n| n.as_str().unwrap()),
                                tool.get("mount_label").map(|l| l.as_str().unwrap()),
                                tool.get("url").map_or("", |u| u.as_str().unwrap()),
                            )
                        })
                        .collect::<Vec<(&str, Option<&str>, &str)>>()
                });

            if vcs_tools.len() > 1 {
                vcs_tools.retain(|tool| {
                    if let Some(url) = tool.2.strip_suffix('/') {
                        !["www", "homepage"].contains(&url.rsplit('/').next().unwrap_or(""))
                    } else {
                        true
                    }
                });
            }

            if vcs_tools.len() > 1 && subproject.is_some() {
                let new_vcs_tools: Vec<(&str, Option<&str>, &str)> = vcs_tools
                    .iter()
                    .filter(|tool| tool.1 == subproject)
                    .cloned()
                    .collect();
                if !new_vcs_tools.is_empty() {
                    vcs_tools = new_vcs_tools;
                }
            }

            if vcs_tools.iter().any(|tool| tool.0 == "cvs") {
                vcs_tools.retain(|tool| tool.0 != "cvs");
            }

            if vcs_tools.len() == 1 {
                let (kind, _, url) = vcs_tools[0];
                match kind {
                    "git" => {
                        let url = format!("https://sourceforge.net/{}", url);
                        let client = reqwest::blocking::Client::new();
                        let response = client
                            .head(url)
                            .header("User-Agent", USER_AGENT)
                            .send()
                            .unwrap();
                        let url = sf_git_extract_url(&response.text().unwrap());
                        if let Some(url) = url {
                            results.push(UpstreamDatum::Repository(url));
                        }
                    }
                    "svn" => {
                        let url = format!("https://svn.code.sf.net/{}", url);
                        results.push(UpstreamDatum::Repository(url));
                    }
                    "hg" => {
                        let url = format!("https://hg.code.sf.net/{}", url);
                        results.push(UpstreamDatum::Repository(url));
                    }
                    "cvs" => {
                        let url = format!(
                            "cvs+pserver://anonymous@{}.cvs.sourceforge.net/cvsroot/{}",
                            sf_project,
                            url.strip_suffix('/')
                                .unwrap_or("")
                                .rsplit('/')
                                .nth(1)
                                .unwrap_or("")
                        );
                        results.push(UpstreamDatum::Repository(url));
                    }
                    "bzr" => {
                        // TODO: Implement Bazaar (BZR) handling
                    }
                    _ => {
                        error!("Unknown VCS kind: {}", kind);
                    }
                }
            } else if vcs_tools.len() > 1 {
                warn!("Multiple possible VCS URLs found");
            }
        }
        None => {
            debug!("No SourceForge metadata found for {}", sf_project);
        }
    }
    results
}

pub fn extract_sf_project_name(url: &str) -> Option<String> {
    let projects_regex = regex!(r"https?://sourceforge\.net/(projects|p)/([^/]+)");
    if let Some(captures) = projects_regex.captures(url) {
        return captures.get(2).map(|m| m.as_str().to_string());
    }

    let sf_regex = regex!(r"https?://(.*).(sf|sourceforge).(net|io)/.*");
    if let Some(captures) = sf_regex.captures(url) {
        return captures.get(1).map(|m| m.as_str().to_string());
    }

    None
}

pub fn extract_pecl_package_name(url: &str) -> Option<String> {
    let pecl_regex = regex!(r"https?://pecl\.php\.net/package/(.*)");
    if let Some(captures) = pecl_regex.captures(url) {
        return captures.get(1).map(|m| m.as_str().to_string());
    }
    None
}

/// Obtain metadata from a URL related to the project
pub fn metadata_from_url(url: &str, origin: Option<&str>) -> Vec<UpstreamDatumWithMetadata> {
    let mut results = Vec::new();
    if let Some(sf_project) = extract_sf_project_name(url) {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::SourceForgeProject(sf_project),
            certainty: Some(Certainty::Certain),
            origin: origin.map(|s| s.to_string()),
        });
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Archive("SourceForge".to_string()),
            certainty: Some(Certainty::Certain),
            origin: origin.map(|s| s.to_string()),
        });
    }

    if let Some(pecl_package) = extract_pecl_package_name(url) {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::PeclPackage(pecl_package),
            certainty: Some(Certainty::Certain),
            origin: origin.map(|s| s.to_string()),
        });
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Archive("Pecl".to_string()),
            certainty: Some(Certainty::Certain),
            origin: origin.map(|s| s.to_string()),
        });
    }

    results
}

pub fn get_repology_metadata(srcname: &str, repo: Option<&str>) -> Option<serde_json::Value> {
    let repo = repo.unwrap_or("debian_unstable");
    let url = format!(
        "https://repology.org/tools/project-by?repo={}&name_type=srcname'
           '&target_page=api_v1_project&name={}",
        repo, srcname
    );

    match load_json_url(&Url::parse(url.as_str()).unwrap(), None) {
        Ok(json) => Some(json),
        Err(HTTPJSONError::Error { status, .. }) if status == 404 => None,
        Err(e) => {
            debug!("Failed to load repology metadata: {:?}", e);
            None
        }
    }
}

pub fn guess_from_path(
    path: &Path,
    trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let basename = path.file_name().and_then(|s| s.to_str());
    let mut ret = Vec::new();
    if let Some(basename_str) = basename {
        let re = regex!(r"(.*)-([0-9.]+)");
        if let Some(captures) = re.captures(basename_str) {
            if let Some(name) = captures.get(1) {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(name.as_str().to_string()),
                    certainty: Some(Certainty::Possible),
                    origin: Some(path.display().to_string()),
                });
            }
            if let Some(version) = captures.get(2) {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(version.as_str().to_string()),
                    certainty: Some(Certainty::Possible),
                    origin: Some(path.display().to_string()),
                });
            }
        } else {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(basename_str.to_string()),
                certainty: Some(Certainty::Possible),
                origin: Some(path.display().to_string()),
            });
        }
    }
    Ok(ret)
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

pub fn py_to_person(py: Python, obj: PyObject) -> PyResult<Person> {
    let name = obj.getattr(py, "name")?.extract::<Option<String>>(py)?;
    let email = obj.getattr(py, "email")?.extract::<Option<String>>(py)?;
    let url = obj.getattr(py, "url")?.extract::<Option<String>>(py)?;

    Ok(Person { name, email, url })
}

pub fn py_to_upstream_datum(py: Python, obj: &PyObject) -> PyResult<UpstreamDatum> {
    let field = obj.getattr(py, "field")?.extract::<String>(py)?;

    let val = obj.getattr(py, "value")?;

    match field.as_str() {
        "Name" => Ok(UpstreamDatum::Name(val.extract::<String>(py)?)),
        "Version" => Ok(UpstreamDatum::Version(val.extract::<String>(py)?)),
        "Homepage" => Ok(UpstreamDatum::Homepage(val.extract::<String>(py)?)),
        "Bug-Database" => Ok(UpstreamDatum::BugDatabase(val.extract::<String>(py)?)),
        "Bug-Submit" => Ok(UpstreamDatum::BugSubmit(val.extract::<String>(py)?)),
        "Contact" => Ok(UpstreamDatum::Contact(val.extract::<String>(py)?)),
        "Repository" => Ok(UpstreamDatum::Repository(val.extract::<String>(py)?)),
        "Repository-Browse" => Ok(UpstreamDatum::RepositoryBrowse(val.extract::<String>(py)?)),
        "License" => Ok(UpstreamDatum::License(val.extract::<String>(py)?)),
        "Description" => Ok(UpstreamDatum::Description(val.extract::<String>(py)?)),
        "Summary" => Ok(UpstreamDatum::Summary(val.extract::<String>(py)?)),
        "Cargo-Crate" => Ok(UpstreamDatum::CargoCrate(val.extract::<String>(py)?)),
        "Security-MD" => Ok(UpstreamDatum::SecurityMD(val.extract::<String>(py)?)),
        "Security-Contact" => Ok(UpstreamDatum::SecurityContact(val.extract::<String>(py)?)),
        "Keywords" => Ok(UpstreamDatum::Keywords(val.extract::<Vec<String>>(py)?)),
        "Copyright" => Ok(UpstreamDatum::Copyright(val.extract::<String>(py)?)),
        "Documentation" => Ok(UpstreamDatum::Documentation(val.extract::<String>(py)?)),
        "Go-Import-Path" => Ok(UpstreamDatum::GoImportPath(val.extract::<String>(py)?)),
        "Download" => Ok(UpstreamDatum::Download(val.extract::<String>(py)?)),
        "Wiki" => Ok(UpstreamDatum::Wiki(val.extract::<String>(py)?)),
        "MailingList" => Ok(UpstreamDatum::MailingList(val.extract::<String>(py)?)),
        "Funding" => Ok(UpstreamDatum::Funding(val.extract::<String>(py)?)),
        "SourceForge-Project" => Ok(UpstreamDatum::SourceForgeProject(
            val.extract::<String>(py)?,
        )),
        "Archive" => Ok(UpstreamDatum::Archive(val.extract::<String>(py)?)),
        "Demo" => Ok(UpstreamDatum::Demo(val.extract::<String>(py)?)),
        "Pecl-Package" => Ok(UpstreamDatum::PeclPackage(val.extract::<String>(py)?)),
        "Author" => Ok(UpstreamDatum::Author(
            val.extract::<Vec<PyObject>>(py)?
                .into_iter()
                .map(|x| py_to_person(py, x))
                .collect::<PyResult<Vec<Person>>>()?,
        )),
        "Maintainer" => Ok(UpstreamDatum::Maintainer(py_to_person(py, val)?)),
        "Changelog" => Ok(UpstreamDatum::Changelog(val.extract::<String>(py)?)),
        _ => Err(PyRuntimeError::new_err(format!("Unknown field: {}", field))),
    }
}

pub fn py_to_upstream_datum_with_metadata(
    py: Python,
    obj: PyObject,
) -> PyResult<UpstreamDatumWithMetadata> {
    let datum = py_to_upstream_datum(py, &obj)?;

    let origin = obj.getattr(py, "origin")?.extract::<Option<String>>(py)?;

    let certainty = obj
        .getattr(py, "certainty")?
        .extract::<Option<String>>(py)?;

    Ok(UpstreamDatumWithMetadata {
        datum,
        certainty: certainty.map(|s| s.parse().unwrap()),
        origin,
    })
}

pub enum ProviderError {
    ParseError(String),
    IoError(std::io::Error),
    Other(String),
}

impl From<std::io::Error> for ProviderError {
    fn from(e: std::io::Error) -> Self {
        ProviderError::IoError(e)
    }
}
