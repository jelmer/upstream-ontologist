use lazy_regex::regex;
use log::{debug, error, warn};
use percent_encoding::utf8_percent_encode;
use pyo3::exceptions::{PyRuntimeError, PyTypeError};
use pyo3::prelude::*;
use reqwest::header::HeaderMap;
use std::str::FromStr;

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use url::Url;

static USER_AGENT: &str = concat!("upstream-ontologist/", env!("CARGO_PKG_VERSION"));

// Too aggressive?
const DEFAULT_URLLIB_TIMEOUT: u64 = 3;

pub mod debian;
pub mod extrapolate;
pub mod providers;
pub mod readme;
pub mod vcs;
pub mod vcs_command;

#[derive(Clone, Copy, Debug, Ord, Eq, PartialOrd, PartialEq)]
pub enum Certainty {
    Certain,
    Confident,
    Likely,
    Possible,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Path(PathBuf),
    Other(String),
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Origin::Path(path) => write!(f, "{}", path.display()),
            Origin::Other(s) => write!(f, "{}", s),
        }
    }
}

impl From<&std::path::Path> for Origin {
    fn from(path: &std::path::Path) -> Self {
        Origin::Path(path.to_path_buf())
    }
}

impl From<std::path::PathBuf> for Origin {
    fn from(path: std::path::PathBuf) -> Self {
        Origin::Path(path)
    }
}

impl ToPyObject for Origin {
    fn to_object(&self, py: Python) -> PyObject {
        match self {
            Origin::Path(path) => path.to_str().unwrap().to_object(py),
            Origin::Other(s) => s.to_object(py),
        }
    }
}

impl IntoPy<PyObject> for Origin {
    fn into_py(self, py: Python) -> PyObject {
        match self {
            Origin::Path(path) => path.to_str().unwrap().to_object(py),
            Origin::Other(s) => s.to_object(py),
        }
    }
}

impl FromPyObject<'_> for Origin {
    fn extract(ob: &PyAny) -> PyResult<Self> {
        if let Ok(path) = ob.extract::<PathBuf>() {
            Ok(Origin::Path(path))
        } else if let Ok(s) = ob.extract::<String>() {
            Ok(Origin::Other(s))
        } else {
            Err(PyTypeError::new_err("expected str or Path"))
        }
    }
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

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub name: Option<String>,
    pub email: Option<String>,
    pub url: Option<String>,
}

impl std::fmt::Display for Person {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name.as_ref().unwrap_or(&"".to_string()))?;
        if let Some(email) = &self.email {
            write!(f, " <{}>", email)?;
        }
        if let Some(url) = &self.url {
            write!(f, " ({})", url)?;
        }
        Ok(())
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
                        Person {
                            name: Some(name),
                            email: Some(email),
                            url: Some(url),
                        }
                    } else {
                        Person {
                            name: Some(p1.to_string()),
                            url: Some(url),
                            ..Default::default()
                        }
                    }
                } else if p2.contains('@') {
                    Person {
                        name: Some(p1.to_string()),
                        email: Some(p2.to_string()),
                        ..Default::default()
                    }
                } else {
                    Person {
                        name: Some(text.to_string()),
                        ..Default::default()
                    }
                }
            } else {
                Person {
                    name: Some(text.to_string()),
                    ..Default::default()
                }
            }
        } else if text.contains('<') {
            if let Some((name, email)) = parseaddr(text.as_str()) {
                return Person {
                    name: Some(name),
                    email: Some(email),
                    ..Default::default()
                };
            } else {
                Person {
                    name: Some(text.to_string()),
                    ..Default::default()
                }
            }
        } else if text.contains('@') && !text.contains(' ') {
            return Person {
                email: Some(text.to_string()),
                ..Default::default()
            };
        } else {
            Person {
                name: Some(text),
                ..Default::default()
            }
        }
    }
}

#[cfg(test)]
mod person_tests {
    use super::*;

    #[test]
    fn test_from_str() {
        assert_eq!(
            Person::from("Foo Bar <foo@example.com>"),
            Person {
                name: Some("Foo Bar".to_string()),
                email: Some("foo@example.com".to_string()),
                url: None
            }
        );
        assert_eq!(
            Person::from("Foo Bar"),
            Person {
                name: Some("Foo Bar".to_string()),
                email: None,
                url: None
            }
        );
        assert_eq!(
            Person::from("foo@example.com"),
            Person {
                name: None,
                email: Some("foo@example.com".to_string()),
                url: None
            }
        );
    }
}

impl ToPyObject for Person {
    fn to_object(&self, py: Python) -> PyObject {
        let m = PyModule::import(py, "upstream_ontologist.guess").unwrap();
        let person_cls = m.getattr("Person").unwrap();
        person_cls
            .call1((self.name.as_ref(), self.email.as_ref(), self.url.as_ref()))
            .unwrap()
            .into_py(py)
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpstreamDatum {
    /// Name of the project
    Name(String),
    /// URL to project homepage
    Homepage(String),
    /// URL to the project's source code repository
    Repository(String),
    /// URL to browse the project's source code repository
    RepositoryBrowse(String),
    /// Long description of the project
    Description(String),
    /// Short summary of the project (one line)
    Summary(String),
    /// License name or SPDX identifier
    License(String),
    /// List of authors
    Author(Vec<Person>),
    /// List of maintainers
    Maintainer(Person),
    /// URL of the project's issue tracker
    BugDatabase(String),
    /// URL to submit a new bug
    BugSubmit(String),
    /// URL to the project's contact page or email address
    Contact(String),
    /// Cargo crate name
    CargoCrate(String),
    /// Name of the security page name
    SecurityMD(String),
    /// URL to the security page or email address
    SecurityContact(String),
    /// Last version of the project
    Version(String),
    /// List of keywords
    Keywords(Vec<String>),
    /// Copyright notice
    Copyright(String),
    /// URL to the project's documentation
    Documentation(String),
    /// Go import path
    GoImportPath(String),
    /// URL to the project's download page
    Download(String),
    /// URL to the project's wiki
    Wiki(String),
    /// URL to the project's mailing list
    MailingList(String),
    /// SourceForge project name
    SourceForgeProject(String),
    Archive(String),
    /// URL to a demo instance
    Demo(String),
    /// PHP PECL package name
    PeclPackage(String),
    /// URL to the funding page
    Funding(String),
    /// URL to the changelog
    Changelog(String),
    /// Haskell package name
    HaskellPackage(String),
    /// Debian ITP (Intent To Package) bug number
    DebianITP(i32),
    /// List of URLs to screenshots
    Screenshots(Vec<String>),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UpstreamDatumWithMetadata {
    pub datum: UpstreamDatum,
    pub origin: Option<Origin>,
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
            UpstreamDatum::HaskellPackage(..) => "Haskell-Package",
            UpstreamDatum::Funding(..) => "Funding",
            UpstreamDatum::Changelog(..) => "Changelog",
            UpstreamDatum::DebianITP(..) => "Debian-ITP",
            UpstreamDatum::Screenshots(..) => "Screenshots",
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
            UpstreamDatum::HaskellPackage(s) => Some(s),
            UpstreamDatum::Author(..) => None,
            UpstreamDatum::Maintainer(..) => None,
            UpstreamDatum::Keywords(..) => None,
            UpstreamDatum::Copyright(c) => Some(c),
            UpstreamDatum::Funding(f) => Some(f),
            UpstreamDatum::Changelog(c) => Some(c),
            UpstreamDatum::Screenshots(..) => None,
            UpstreamDatum::DebianITP(_c) => None,
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

impl std::fmt::Display for UpstreamDatum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpstreamDatum::Name(s) => write!(f, "Name: {}", s),
            UpstreamDatum::Homepage(s) => write!(f, "Homepage: {}", s),
            UpstreamDatum::Repository(s) => write!(f, "Repository: {}", s),
            UpstreamDatum::RepositoryBrowse(s) => write!(f, "RepositoryBrowse: {}", s),
            UpstreamDatum::Description(s) => write!(f, "Description: {}", s),
            UpstreamDatum::Summary(s) => write!(f, "Summary: {}", s),
            UpstreamDatum::License(s) => write!(f, "License: {}", s),
            UpstreamDatum::BugDatabase(s) => write!(f, "BugDatabase: {}", s),
            UpstreamDatum::BugSubmit(s) => write!(f, "BugSubmit: {}", s),
            UpstreamDatum::Contact(s) => write!(f, "Contact: {}", s),
            UpstreamDatum::CargoCrate(s) => write!(f, "CargoCrate: {}", s),
            UpstreamDatum::SecurityMD(s) => write!(f, "SecurityMD: {}", s),
            UpstreamDatum::SecurityContact(s) => write!(f, "SecurityContact: {}", s),
            UpstreamDatum::Version(s) => write!(f, "Version: {}", s),
            UpstreamDatum::Documentation(s) => write!(f, "Documentation: {}", s),
            UpstreamDatum::GoImportPath(s) => write!(f, "GoImportPath: {}", s),
            UpstreamDatum::Download(s) => write!(f, "Download: {}", s),
            UpstreamDatum::Wiki(s) => write!(f, "Wiki: {}", s),
            UpstreamDatum::MailingList(s) => write!(f, "MailingList: {}", s),
            UpstreamDatum::SourceForgeProject(s) => write!(f, "SourceForgeProject: {}", s),
            UpstreamDatum::Archive(s) => write!(f, "Archive: {}", s),
            UpstreamDatum::Demo(s) => write!(f, "Demo: {}", s),
            UpstreamDatum::PeclPackage(s) => write!(f, "PeclPackage: {}", s),
            UpstreamDatum::Author(authors) => {
                write!(
                    f,
                    "Author: {}",
                    authors
                        .iter()
                        .map(|a| a.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            UpstreamDatum::Maintainer(maintainer) => {
                write!(f, "Maintainer: {}", maintainer)
            }
            UpstreamDatum::Keywords(keywords) => {
                write!(
                    f,
                    "Keywords: {}",
                    keywords
                        .iter()
                        .map(|a| a.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            UpstreamDatum::Copyright(s) => {
                write!(f, "Copyright: {}", s)
            }
            UpstreamDatum::Funding(s) => {
                write!(f, "Funding: {}", s)
            }
            UpstreamDatum::Changelog(s) => {
                write!(f, "Changelog: {}", s)
            }
            UpstreamDatum::DebianITP(s) => {
                write!(f, "DebianITP: {}", s)
            }
            UpstreamDatum::HaskellPackage(p) => {
                write!(f, "HaskellPackage: {}", p)
            }
            UpstreamDatum::Screenshots(s) => {
                write!(f, "Screenshots: {}", s.join(", "))
            }
        }
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

    pub fn get(&self, field: &str) -> Option<&UpstreamDatumWithMetadata> {
        self.0.iter().find(|d| d.datum.field() == field)
    }

    pub fn insert(&mut self, datum: UpstreamDatumWithMetadata) {
        self.0.push(datum);
    }

    pub fn contains_key(&self, field: &str) -> bool {
        self.get(field).is_some()
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

impl ToPyObject for UpstreamDatumWithMetadata {
    fn to_object(&self, py: Python) -> PyObject {
        let m = PyModule::import(py, "upstream_ontologist.guess").unwrap();

        let cls = m.getattr("UpstreamDatum").unwrap();

        let (field, py_datum) = self
            .datum
            .to_object(py)
            .extract::<(String, PyObject)>(py)
            .unwrap();

        let kwargs = pyo3::types::PyDict::new(py);
        kwargs
            .set_item("certainty", self.certainty.map(|x| x.to_string()))
            .unwrap();
        kwargs.set_item("origin", self.origin.as_ref()).unwrap();

        let datum = cls.call((field, py_datum), Some(kwargs)).unwrap();

        datum.to_object(py)
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
                "Haskell-Package" => {
                    UpstreamDatum::HaskellPackage(value.extract::<String>(py).unwrap())
                }
                "Debian-ITP" => UpstreamDatum::DebianITP(value.extract::<i32>(py).unwrap()),
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

#[derive(Debug)]
pub enum HTTPJSONError {
    HTTPError(reqwest::Error),
    Error {
        url: reqwest::Url,
        status: u16,
        response: reqwest::blocking::Response,
    },
}

impl std::fmt::Display for HTTPJSONError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            HTTPJSONError::HTTPError(e) => write!(f, "{}", e),
            HTTPJSONError::Error {
                url,
                status,
                response: _,
            } => write!(f, "HTTP error {} for {}:", status, url,),
        }
    }
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
                        origin: Some(path.into()),
                    });
                }
            }
            "name" => {
                if let Some(name) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Name(name.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
            "version" => {
                if let Some(version) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Version(version.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
            "url" => {
                if let Some(url) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Homepage(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
            "license" => {
                if let Some(license) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::License(license.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
            "source" => {
                if let Some(repository) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Repository(repository.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
            "summary" => {
                if let Some(summary) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Summary(summary.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
            "issues_url" => {
                if let Some(issues_url) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::BugDatabase(issues_url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
            "project_page" => {
                if let Some(project_page) = value.as_str() {
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Homepage(project_page.to_string()),
                        certainty: Some(Certainty::Likely),
                        origin: Some(path.into()),
                    });
                }
            }
            "author" => {
                if let Some(author_value) = value.as_str() {
                    let author = Person::from(author_value);
                    upstream_data.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Author(vec![author]),
                        certainty: Some(Certainty::Likely),
                        origin: Some(path.into()),
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
                        origin: Some(path.into()),
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

    fn bug_database_url_from_bug_submit_url(&self, _url: &Url) -> Option<Url> {
        None
    }

    fn bug_submit_url_from_bug_database_url(&self, _url: &Url) -> Option<Url> {
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

    fn bug_database_from_issue_url(&self, _url: &Url) -> Option<Url> {
        None
    }

    fn bug_database_url_from_repo_url(&self, _url: &Url) -> Option<Url> {
        None
    }

    fn repo_url_from_merge_request_url(&self, _url: &Url) -> Option<Url> {
        None
    }

    fn extend_metadata(
        &self,
        _metadata: &mut Vec<UpstreamDatumWithMetadata>,
        _project: &str,
        _max_certainty: Certainty,
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
                origin: Some(path.into()),
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
            origin: Some(Origin::Other("environment".to_string())),
        });
    }
    Ok(results)
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
    _field_certainty: Certainty,
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
        let subproject = find_datum(metadata, "Name").and_then(|f| match f.datum {
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

pub fn extract_hackage_package(url: &str) -> Option<String> {
    let hackage_regex = regex!(r"https?://hackage\.haskell\.org/package/([^/]+)/.*");
    if let Some(captures) = hackage_regex.captures(url) {
        return captures.get(1).map(|m| m.as_str().to_string());
    }
    None
}

/// Obtain metadata from a URL related to the project
pub fn metadata_from_url(url: &str, origin: &Origin) -> Vec<UpstreamDatumWithMetadata> {
    let mut results = Vec::new();
    if let Some(sf_project) = extract_sf_project_name(url) {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::SourceForgeProject(sf_project),
            certainty: Some(Certainty::Certain),
            origin: Some(origin.clone()),
        });
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Archive("SourceForge".to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(origin.clone()),
        });
    }

    if let Some(pecl_package) = extract_pecl_package_name(url) {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::PeclPackage(pecl_package),
            certainty: Some(Certainty::Certain),
            origin: Some(origin.clone()),
        });
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Archive("Pecl".to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(origin.clone()),
        });
    }

    if let Some(haskell_package) = extract_hackage_package(url) {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::HaskellPackage(haskell_package),
            certainty: Some(Certainty::Certain),
            origin: Some(origin.clone()),
        });
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Archive("Hackage".to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(origin.clone()),
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
    _trust_package: bool,
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
                    origin: Some(path.into()),
                });
            }
            if let Some(version) = captures.get(2) {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(version.as_str().to_string()),
                    certainty: Some(Certainty::Possible),
                    origin: Some(path.into()),
                });
            }
        } else {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(basename_str.to_string()),
                certainty: Some(Certainty::Possible),
                origin: Some(path.into()),
            });
        }
    }
    Ok(ret)
}

impl FromPyObject<'_> for UpstreamDatum {
    fn extract(obj: &PyAny) -> PyResult<Self> {
        let (field, val): (String, &PyAny) = if let Ok((field, val)) =
            obj.extract::<(String, &PyAny)>()
        {
            (field, val)
        } else if let Ok(datum) = obj.getattr("datum") {
            let field = datum.getattr("field")?.extract::<String>()?;
            let val = datum.getattr("value")?;
            (field, val)
        } else if obj.hasattr("field")? && obj.hasattr("value")? {
            let field = obj.getattr("field")?.extract::<String>()?;
            let val = obj.getattr("value")?;
            (field, val)
        } else {
            return Err(PyTypeError::new_err((
                format!("Expected a tuple of (field, value) or an object with field and value attributesm, found {:?}", obj),
            )));
        };

        match field.as_str() {
            "Name" => Ok(UpstreamDatum::Name(val.extract::<String>()?)),
            "Version" => Ok(UpstreamDatum::Version(val.extract::<String>()?)),
            "Homepage" => Ok(UpstreamDatum::Homepage(val.extract::<String>()?)),
            "Bug-Database" => Ok(UpstreamDatum::BugDatabase(val.extract::<String>()?)),
            "Bug-Submit" => Ok(UpstreamDatum::BugSubmit(val.extract::<String>()?)),
            "Contact" => Ok(UpstreamDatum::Contact(val.extract::<String>()?)),
            "Repository" => Ok(UpstreamDatum::Repository(val.extract::<String>()?)),
            "Repository-Browse" => Ok(UpstreamDatum::RepositoryBrowse(val.extract::<String>()?)),
            "License" => Ok(UpstreamDatum::License(val.extract::<String>()?)),
            "Description" => Ok(UpstreamDatum::Description(val.extract::<String>()?)),
            "Summary" => Ok(UpstreamDatum::Summary(val.extract::<String>()?)),
            "Cargo-Crate" => Ok(UpstreamDatum::CargoCrate(val.extract::<String>()?)),
            "Security-MD" => Ok(UpstreamDatum::SecurityMD(val.extract::<String>()?)),
            "Security-Contact" => Ok(UpstreamDatum::SecurityContact(val.extract::<String>()?)),
            "Keywords" => Ok(UpstreamDatum::Keywords(val.extract::<Vec<String>>()?)),
            "Copyright" => Ok(UpstreamDatum::Copyright(val.extract::<String>()?)),
            "Documentation" => Ok(UpstreamDatum::Documentation(val.extract::<String>()?)),
            "Go-Import-Path" => Ok(UpstreamDatum::GoImportPath(val.extract::<String>()?)),
            "Download" => Ok(UpstreamDatum::Download(val.extract::<String>()?)),
            "Wiki" => Ok(UpstreamDatum::Wiki(val.extract::<String>()?)),
            "MailingList" => Ok(UpstreamDatum::MailingList(val.extract::<String>()?)),
            "Funding" => Ok(UpstreamDatum::Funding(val.extract::<String>()?)),
            "SourceForge-Project" => {
                Ok(UpstreamDatum::SourceForgeProject(val.extract::<String>()?))
            }
            "Archive" => Ok(UpstreamDatum::Archive(val.extract::<String>()?)),
            "Demo" => Ok(UpstreamDatum::Demo(val.extract::<String>()?)),
            "Pecl-Package" => Ok(UpstreamDatum::PeclPackage(val.extract::<String>()?)),
            "Haskell-Package" => Ok(UpstreamDatum::HaskellPackage(val.extract::<String>()?)),
            "Author" => Ok(UpstreamDatum::Author(val.extract::<Vec<Person>>()?)),
            "Maintainer" => Ok(UpstreamDatum::Maintainer(val.extract::<Person>()?)),
            "Changelog" => Ok(UpstreamDatum::Changelog(val.extract::<String>()?)),
            "Screenshots" => Ok(UpstreamDatum::Screenshots(val.extract::<Vec<String>>()?)),
            _ => Err(PyRuntimeError::new_err(format!("Unknown field: {}", field))),
        }
    }
}

impl ToPyObject for UpstreamDatum {
    fn to_object(&self, py: Python) -> PyObject {
        (
            self.field().to_string(),
            match self {
                UpstreamDatum::Name(n) => n.into_py(py),
                UpstreamDatum::Version(v) => v.into_py(py),
                UpstreamDatum::Contact(c) => c.into_py(py),
                UpstreamDatum::Summary(s) => s.into_py(py),
                UpstreamDatum::License(l) => l.into_py(py),
                UpstreamDatum::Homepage(h) => h.into_py(py),
                UpstreamDatum::Description(d) => d.into_py(py),
                UpstreamDatum::BugDatabase(b) => b.into_py(py),
                UpstreamDatum::BugSubmit(b) => b.into_py(py),
                UpstreamDatum::Repository(r) => r.into_py(py),
                UpstreamDatum::RepositoryBrowse(r) => r.into_py(py),
                UpstreamDatum::SecurityMD(s) => s.into_py(py),
                UpstreamDatum::SecurityContact(s) => s.into_py(py),
                UpstreamDatum::CargoCrate(c) => c.into_py(py),
                UpstreamDatum::Keywords(ks) => ks.to_object(py),
                UpstreamDatum::Copyright(c) => c.into_py(py),
                UpstreamDatum::Documentation(a) => a.into_py(py),
                UpstreamDatum::GoImportPath(ip) => ip.into_py(py),
                UpstreamDatum::Archive(a) => a.into_py(py),
                UpstreamDatum::Demo(d) => d.into_py(py),
                UpstreamDatum::Maintainer(m) => m.to_object(py),
                UpstreamDatum::Author(a) => a.to_object(py),
                UpstreamDatum::Wiki(w) => w.into_py(py),
                UpstreamDatum::Download(d) => d.into_py(py),
                UpstreamDatum::MailingList(m) => m.into_py(py),
                UpstreamDatum::SourceForgeProject(m) => m.into_py(py),
                UpstreamDatum::PeclPackage(p) => p.into_py(py),
                UpstreamDatum::Funding(p) => p.into_py(py),
                UpstreamDatum::Changelog(c) => c.into_py(py),
                UpstreamDatum::HaskellPackage(p) => p.into_py(py),
                UpstreamDatum::DebianITP(i) => i.into_py(py),
                UpstreamDatum::Screenshots(s) => s.to_object(py),
            },
        )
            .to_object(py)
    }
}

impl FromPyObject<'_> for UpstreamDatumWithMetadata {
    fn extract(obj: &PyAny) -> PyResult<Self> {
        let certainty = obj.getattr("certainty")?.extract::<Option<String>>()?;
        let origin = obj.getattr("origin")?.extract::<Option<Origin>>()?;
        let datum = if obj.hasattr("datum")? {
            obj.getattr("datum")?.extract::<UpstreamDatum>()
        } else {
            obj.extract::<UpstreamDatum>()
        }?;

        Ok(UpstreamDatumWithMetadata {
            datum,
            certainty: certainty.map(|s| s.parse().unwrap()),
            origin,
        })
    }
}

#[derive(Debug)]
pub enum ProviderError {
    ParseError(String),
    IoError(std::io::Error),
    Other(String),
    HttpJsonError(HTTPJSONError),
    Python(PyErr),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ProviderError::ParseError(e) => write!(f, "Parse error: {}", e),
            ProviderError::IoError(e) => write!(f, "IO error: {}", e),
            ProviderError::Other(e) => write!(f, "Other error: {}", e),
            ProviderError::HttpJsonError(e) => write!(f, "HTTP JSON error: {}", e),
            ProviderError::Python(e) => write!(f, "Python error: {}", e),
        }
    }
}

impl std::error::Error for ProviderError {}

impl From<HTTPJSONError> for ProviderError {
    fn from(e: HTTPJSONError) -> Self {
        ProviderError::HttpJsonError(e)
    }
}

impl From<std::io::Error> for ProviderError {
    fn from(e: std::io::Error) -> Self {
        ProviderError::IoError(e)
    }
}

#[cfg(feature = "pyo3")]
pyo3::create_exception!(
    upstream_ontologist,
    ParseError,
    pyo3::exceptions::PyException
);

#[cfg(feature = "pyo3")]
impl From<ProviderError> for PyErr {
    fn from(e: ProviderError) -> PyErr {
        match e {
            ProviderError::IoError(e) => e.into(),
            ProviderError::ParseError(e) => ParseError::new_err((e,)),
            ProviderError::Other(e) => PyRuntimeError::new_err((e,)),
            ProviderError::HttpJsonError(e) => PyRuntimeError::new_err((e.to_string(),)),
            ProviderError::Python(e) => e,
        }
    }
}

#[derive(Debug)]
pub struct UpstreamPackage {
    pub family: String,
    pub name: String,
}

impl FromPyObject<'_> for UpstreamPackage {
    fn extract(obj: &PyAny) -> PyResult<Self> {
        let family = obj.getattr("family")?.extract::<String>()?;
        let name = obj.getattr("name")?.extract::<String>()?;
        Ok(UpstreamPackage { family, name })
    }
}

impl ToPyObject for UpstreamPackage {
    fn to_object(&self, py: Python) -> PyObject {
        let dict = pyo3::types::PyDict::new(py);
        dict.set_item("family", self.family.clone()).unwrap();
        dict.set_item("name", self.name.clone()).unwrap();
        dict.into()
    }
}

#[derive(Debug)]
pub struct UpstreamVersion(String);

impl FromPyObject<'_> for UpstreamVersion {
    fn extract(obj: &PyAny) -> PyResult<Self> {
        let version = obj.extract::<String>()?;
        Ok(UpstreamVersion(version))
    }
}

impl ToPyObject for UpstreamVersion {
    fn to_object(&self, py: Python) -> PyObject {
        self.0.to_object(py)
    }
}

#[derive(Debug)]
pub struct GuesserSettings {
    pub trust_package: bool,
}

pub struct UpstreamMetadataGuesser {
    pub name: std::path::PathBuf,
    pub guess:
        Box<dyn FnOnce(&GuesserSettings) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError>>,
}

impl std::fmt::Debug for UpstreamMetadataGuesser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpstreamMetadataGuesser")
            .field("name", &self.name)
            .finish()
    }
}

const STATIC_GUESSERS: &[(
    &str,
    fn(&std::path::Path, bool) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError>,
)] = &[
    (
        "debian/watch",
        crate::providers::debian::guess_from_debian_watch,
    ),
    (
        "debian/control",
        crate::providers::debian::guess_from_debian_control,
    ),
    (
        "debian/changelog",
        crate::providers::debian::guess_from_debian_changelog,
    ),
    (
        "debian/rules",
        crate::providers::debian::guess_from_debian_rules,
    ),
    ("PKG-INFO", crate::providers::python::guess_from_pkg_info),
    (
        "package.json",
        crate::providers::package_json::guess_from_package_json,
    ),
    (
        "composer.json",
        crate::providers::composer_json::guess_from_composer_json,
    ),
    (
        "package.xml",
        crate::providers::package_xml::guess_from_package_xml,
    ),
    (
        "package.yaml",
        crate::providers::package_yaml::guess_from_package_yaml,
    ),
    ("dist.ini", crate::providers::perl::guess_from_dist_ini),
    (
        "debian/copyright",
        crate::providers::debian::guess_from_debian_copyright,
    ),
    ("META.json", crate::providers::perl::guess_from_meta_json),
    ("MYMETA.json", crate::providers::perl::guess_from_meta_json),
    ("META.yml", crate::providers::perl::guess_from_meta_yml),
    ("MYMETA.yml", crate::providers::perl::guess_from_meta_yml),
    (
        "configure",
        crate::providers::autoconf::guess_from_configure,
    ),
    ("DESCRIPTION", crate::providers::r::guess_from_r_description),
    ("Cargo.toml", crate::providers::rust::guess_from_cargo),
    ("pom.xml", crate::providers::maven::guess_from_pom_xml),
    (".git/config", crate::providers::git::guess_from_git_config),
    (
        "debian/get-orig-source.sh",
        crate::vcs_command::guess_from_get_orig_source,
    ),
    (
        "pyproject.toml",
        crate::providers::python::guess_from_pyproject_toml,
    ),
    ("setup.cfg", crate::providers::python::guess_from_setup_cfg),
    ("go.mod", crate::providers::go::guess_from_go_mod),
    (
        "Makefile.PL",
        crate::providers::perl::guess_from_makefile_pl,
    ),
    ("wscript", crate::providers::waf::guess_from_wscript),
    ("AUTHORS", crate::providers::authors::guess_from_authors),
    ("INSTALL", crate::providers::guess_from_install),
    (
        "pubspec.yaml",
        crate::providers::pubspec::guess_from_pubspec_yaml,
    ),
    (
        "pubspec.yml",
        crate::providers::pubspec::guess_from_pubspec_yaml,
    ),
    ("meson.build", crate::providers::meson::guess_from_meson),
    ("metadata.json", crate::guess_from_metadata_json),
    (".travis.yml", crate::guess_from_travis_yml),
];

fn find_guessers(path: &std::path::Path) -> Vec<UpstreamMetadataGuesser> {
    let mut candidates: Vec<(
        String,
        Box<
            dyn FnOnce(
                &std::path::Path,
                &GuesserSettings,
            ) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError>,
        >,
    )> = Vec::new();

    let path = path.canonicalize().unwrap();

    for (name, cb) in STATIC_GUESSERS {
        let subpath = path.join(name);
        if subpath.exists() {
            candidates.push((
                name.to_string(),
                Box::new(move |_path, s: &GuesserSettings| cb(&subpath, s.trust_package)),
            ));
        }
    }

    for name in ["SECURITY.md", ".github/SECURITY.md", "docs/SECURITY.md"].iter() {
        if path.join(name).exists() {
            candidates.push((
                name.to_string(),
                Box::new(move |path, s: &GuesserSettings| {
                    crate::providers::security_md::guess_from_security_md(
                        name,
                        path,
                        s.trust_package,
                    )
                }),
            ));
        }
    }

    let mut found_pkg_info = path.join("PKG-INFO").exists();
    for entry in std::fs::read_dir(&path).unwrap() {
        let entry = entry.unwrap();
        let filename = entry.file_name().to_string_lossy().to_string();
        if filename.ends_with(".egg-info") {
            candidates.push((
                format!("{}/PKG-INFO", filename),
                Box::new(move |_path, s| {
                    crate::providers::python::guess_from_pkg_info(
                        entry.path().join("PKG-INFO").as_path(),
                        s.trust_package,
                    )
                }),
            ));
            found_pkg_info = true;
        } else if filename.ends_with(".dist-info") {
            candidates.push((
                format!("{}/METADATA", filename),
                Box::new(move |_path, s| {
                    crate::providers::python::guess_from_pkg_info(
                        entry.path().join("PKG-INFO").as_path(),
                        s.trust_package,
                    )
                }),
            ));
            found_pkg_info = true;
        }
    }

    if !found_pkg_info && path.join("setup.py").exists() {
        candidates.push((
            "setup.py".to_string(),
            Box::new(|path, s| {
                crate::providers::python::guess_from_setup_py(path, s.trust_package)
            }),
        ));
    }

    for entry in std::fs::read_dir(&path).unwrap() {
        let entry = entry.unwrap();

        if entry.file_name().to_string_lossy().ends_with(".gemspec") {
            candidates.push((
                entry.file_name().to_string_lossy().to_string(),
                Box::new(move |_path, s| {
                    crate::providers::ruby::guess_from_gemspec(
                        entry.path().as_path(),
                        s.trust_package,
                    )
                }),
            ));
        }
    }

    // TODO(jelmer): Perhaps scan all directories if no other primary project information file has been found?
    for entry in std::fs::read_dir(&path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if entry.file_type().unwrap().is_dir() {
            let description_name = format!("{}/DESCRIPTION", entry.file_name().to_string_lossy());
            if path.join(&description_name).exists() {
                candidates.push((
                    description_name,
                    Box::new(move |_path, s| {
                        crate::providers::r::guess_from_r_description(
                            entry.path().as_path(),
                            s.trust_package,
                        )
                    }),
                ));
            }
        }
    }

    let mut doap_filenames = std::fs::read_dir(&path)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            let filename = entry.file_name().to_string_lossy().to_string();
            if filename.ends_with(".doap")
                || (filename.ends_with(".xml") && filename.starts_with("doap_XML_"))
            {
                Some(entry.file_name())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if doap_filenames.len() == 1 {
        let doap_filename = doap_filenames.remove(0);
        candidates.push((
            doap_filename.to_string_lossy().to_string(),
            Box::new(|path, s| crate::providers::doap::guess_from_doap(path, s.trust_package)),
        ));
    } else if doap_filenames.len() > 1 {
        log::warn!(
            "Multiple DOAP files found: {:?}, ignoring all.",
            doap_filenames
        );
    }

    let mut metainfo_filenames = std::fs::read_dir(&path)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            if entry
                .file_name()
                .to_string_lossy()
                .ends_with(".metainfo.xml")
            {
                Some(entry.file_name())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if metainfo_filenames.len() == 1 {
        let metainfo_filename = metainfo_filenames.remove(0);
        candidates.push((
            metainfo_filename.to_string_lossy().to_string(),
            Box::new(|path, s| {
                crate::providers::metainfo::guess_from_metainfo(path, s.trust_package)
            }),
        ));
    } else if metainfo_filenames.len() > 1 {
        log::warn!(
            "Multiple metainfo files found: {:?}, ignoring all.",
            metainfo_filenames
        );
    }

    let mut cabal_filenames = std::fs::read_dir(&path)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            if entry.file_name().to_string_lossy().ends_with(".cabal") {
                Some(entry.file_name())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if cabal_filenames.len() == 1 {
        let cabal_filename = cabal_filenames.remove(0);
        candidates.push((
            cabal_filename.to_string_lossy().to_string(),
            Box::new(|path, s| crate::providers::haskell::guess_from_cabal(path, s.trust_package)),
        ));
    } else if cabal_filenames.len() > 1 {
        log::warn!(
            "Multiple cabal files found: {:?}, ignoring all.",
            cabal_filenames
        );
    }

    let readme_filenames = std::fs::read_dir(&path)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            let filename = entry.file_name().to_string_lossy().to_string();
            if !(filename.to_lowercase().starts_with("readme")
                || filename.to_lowercase().starts_with("hacking")
                || filename.to_lowercase().starts_with("contributing"))
            {
                return None;
            }

            if filename.ends_with('~') {
                return None;
            }

            let extension = entry
                .path()
                .extension()
                .map(|s| s.to_string_lossy().to_string());

            if extension.as_deref() == Some("html")
                || extension.as_deref() == Some("pdf")
                || extension.as_deref() == Some("xml")
            {
                return None;
            }
            Some(entry.file_name())
        })
        .collect::<Vec<_>>();

    for filename in readme_filenames {
        candidates.push((
            filename.to_string_lossy().to_string(),
            Box::new(|path, s| crate::readme::guess_from_readme(path, s.trust_package)),
        ));
    }

    let mut nuspec_filenames = std::fs::read_dir(&path)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            if entry.file_name().to_string_lossy().ends_with(".nuspec") {
                Some(entry.file_name())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if nuspec_filenames.len() == 1 {
        let nuspec_filename = nuspec_filenames.remove(0);
        candidates.push((
            nuspec_filename.to_string_lossy().to_string(),
            Box::new(|path, s| crate::providers::nuspec::guess_from_nuspec(path, s.trust_package)),
        ));
    } else if nuspec_filenames.len() > 1 {
        log::warn!(
            "Multiple nuspec files found: {:?}, ignoring all.",
            nuspec_filenames
        );
    }

    let mut opam_filenames = std::fs::read_dir(&path)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            if entry.file_name().to_string_lossy().ends_with(".opam") {
                Some(entry.file_name())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if opam_filenames.len() == 1 {
        let opam_filename = opam_filenames.remove(0);
        candidates.push((
            opam_filename.to_string_lossy().to_string(),
            Box::new(|path, s| crate::providers::ocaml::guess_from_opam(path, s.trust_package)),
        ));
    } else if opam_filenames.len() > 1 {
        log::warn!(
            "Multiple opam files found: {:?}, ignoring all.",
            opam_filenames
        );
    }

    let debian_patches = match std::fs::read_dir(path.join("debian").join("patches")) {
        Ok(patches) => patches
            .filter_map(|entry| {
                let entry = entry.unwrap();
                if entry.file_name().to_string_lossy().ends_with(".patch") {
                    Some(format!(
                        "debian/patches/{}",
                        entry.file_name().to_string_lossy()
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>(),
        Err(_) => Vec::new(),
    };

    for filename in debian_patches {
        candidates.push((
            filename.clone(),
            Box::new(|path, s| {
                crate::providers::debian::guess_from_debian_patch(path, s.trust_package)
            }),
        ));
    }

    candidates.push((
        "environment".to_string(),
        Box::new(|_path, _| crate::guess_from_environment()),
    ));
    candidates.push((
        ".".to_string(),
        Box::new(|path, s| crate::guess_from_path(path, s.trust_package)),
    ));

    candidates
        .into_iter()
        .filter_map(|(name, cb)| {
            assert!(
                name.len() > 0 && !name.starts_with('/'),
                "invalid name: {}",
                name
            );
            let path = path.join(name);
            Some(UpstreamMetadataGuesser {
                name: path.clone(),
                guess: Box::new(move |s| cb(&path, s)),
            })
        })
        .collect()
}

pub struct UpstreamMetadataScanner {
    path: std::path::PathBuf,
    config: GuesserSettings,
    pending: Vec<UpstreamDatumWithMetadata>,
    guessers: Vec<UpstreamMetadataGuesser>,
}

impl UpstreamMetadataScanner {
    pub fn from_path(path: &std::path::Path, trust_package: Option<bool>) -> Self {
        let trust_package = trust_package.unwrap_or(false);

        let guessers = find_guessers(path);

        Self {
            path: path.to_path_buf(),
            pending: Vec::new(),
            config: GuesserSettings { trust_package },
            guessers,
        }
    }
}

impl Iterator for UpstreamMetadataScanner {
    type Item = Result<UpstreamDatumWithMetadata, ProviderError>;

    fn next(&mut self) -> Option<Result<UpstreamDatumWithMetadata, ProviderError>> {
        loop {
            if !self.pending.is_empty() {
                return Some(Ok(self.pending.remove(0)));
            }

            if self.guessers.is_empty() {
                return None;
            }

            let guesser = self.guessers.remove(0);

            let guess = (guesser.guess)(&self.config);
            match guess {
                Ok(entries) => {
                    self.pending.extend(entries.into_iter().map(|mut e| {
                        e.origin = e
                            .origin
                            .or(Some(Origin::Other(guesser.name.display().to_string())));
                        e
                    }));
                }
                Err(e) => {
                    return Some(Err(e));
                }
            }
        }
    }
}

pub fn guess_upstream_info(
    path: &std::path::Path,
    trust_package: Option<bool>,
) -> impl Iterator<Item = Result<UpstreamDatumWithMetadata, ProviderError>> {
    UpstreamMetadataScanner::from_path(path, trust_package)
}
