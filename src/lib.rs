use lazy_regex::regex;
use log::{debug, error, warn};
use percent_encoding::utf8_percent_encode;
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use reqwest::header::HeaderMap;
use serde::ser::SerializeSeq;
use std::cmp::Ordering;
use std::str::FromStr;

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use url::Url;

static USER_AGENT: &str = concat!("upstream-ontologist/", env!("CARGO_PKG_VERSION"));

pub mod debian;
pub mod extrapolate;
pub mod homepage;
pub mod http;
pub mod providers;
pub mod readme;
pub mod vcs;
pub mod vcs_command;

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum Certainty {
    Possible,
    Likely,
    Confident,
    Certain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Path(PathBuf),
    Url(url::Url),
    Other(String),
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Origin::Path(path) => write!(f, "{}", path.display()),
            Origin::Url(url) => write!(f, "{}", url),
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

impl From<url::Url> for Origin {
    fn from(url: url::Url) -> Self {
        Origin::Url(url)
    }
}

impl ToPyObject for Origin {
    fn to_object(&self, py: Python) -> PyObject {
        match self {
            Origin::Path(path) => path.to_str().unwrap().to_object(py),
            Origin::Url(url) => url.to_string().to_object(py),
            Origin::Other(s) => s.to_object(py),
        }
    }
}

impl IntoPy<PyObject> for Origin {
    fn into_py(self, py: Python) -> PyObject {
        match self {
            Origin::Path(path) => path.to_str().unwrap().to_object(py),
            Origin::Url(url) => url.to_string().to_object(py),
            Origin::Other(s) => s.to_object(py),
        }
    }
}

impl FromPyObject<'_> for Origin {
    fn extract_bound(ob: &Bound<PyAny>) -> PyResult<Self> {
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

impl std::fmt::Display for Certainty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Certainty::Certain => write!(f, "certain"),
            Certainty::Confident => write!(f, "confident"),
            Certainty::Likely => write!(f, "likely"),
            Certainty::Possible => write!(f, "possible"),
        }
    }
}

#[cfg(feature = "pyo3")]
impl FromPyObject<'_> for Certainty {
    fn extract_bound(ob: &Bound<PyAny>) -> PyResult<Self> {
        let o = ob.extract::<&str>()?;
        o.parse().map_err(PyValueError::new_err)
    }
}

#[derive(Default, Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
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
                email: Some(text),
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
        let m = PyModule::import_bound(py, "upstream_ontologist").unwrap();
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
    fn extract_bound(ob: &Bound<PyAny>) -> PyResult<Self> {
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
    /// Name of registry
    Registry(Vec<(String, String)>),
    /// Recommended way to cite the software
    CiteAs(String),
    /// Link for donations (e.g. Paypal, Libera, etc)
    Donation(String),
    /// Link to a life instance of the webservice
    Webservice(String),
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
            UpstreamDatum::Registry(..) => "Registry",
            UpstreamDatum::CiteAs(..) => "Cite-As",
            UpstreamDatum::Donation(..) => "Donation",
            UpstreamDatum::Webservice(..) => "Webservice",
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
            UpstreamDatum::CiteAs(c) => Some(c),
            UpstreamDatum::Registry(_) => None,
            UpstreamDatum::Donation(d) => Some(d),
            UpstreamDatum::Webservice(w) => Some(w),
        }
    }

    pub fn to_url(&self) -> Option<url::Url> {
        match self {
            UpstreamDatum::Name(..) => None,
            UpstreamDatum::Homepage(s) => Some(s.parse().ok()?),
            UpstreamDatum::Repository(s) => Some(s.parse().ok()?),
            UpstreamDatum::RepositoryBrowse(s) => Some(s.parse().ok()?),
            UpstreamDatum::Description(..) => None,
            UpstreamDatum::Summary(..) => None,
            UpstreamDatum::License(..) => None,
            UpstreamDatum::BugDatabase(s) => Some(s.parse().ok()?),
            UpstreamDatum::BugSubmit(s) => Some(s.parse().ok()?),
            UpstreamDatum::Contact(..) => None,
            UpstreamDatum::CargoCrate(s) => Some(s.parse().ok()?),
            UpstreamDatum::SecurityMD(..) => None,
            UpstreamDatum::SecurityContact(..) => None,
            UpstreamDatum::Version(..) => None,
            UpstreamDatum::Documentation(s) => Some(s.parse().ok()?),
            UpstreamDatum::GoImportPath(_s) => None,
            UpstreamDatum::Download(s) => Some(s.parse().ok()?),
            UpstreamDatum::Wiki(s) => Some(s.parse().ok()?),
            UpstreamDatum::MailingList(s) => Some(s.parse().ok()?),
            UpstreamDatum::SourceForgeProject(s) => Some(s.parse().ok()?),
            UpstreamDatum::Archive(s) => Some(s.parse().ok()?),
            UpstreamDatum::Demo(s) => Some(s.parse().ok()?),
            UpstreamDatum::PeclPackage(_s) => None,
            UpstreamDatum::HaskellPackage(_s) => None,
            UpstreamDatum::Author(..) => None,
            UpstreamDatum::Maintainer(..) => None,
            UpstreamDatum::Keywords(..) => None,
            UpstreamDatum::Copyright(..) => None,
            UpstreamDatum::Funding(s) => Some(s.parse().ok()?),
            UpstreamDatum::Changelog(s) => Some(s.parse().ok()?),
            UpstreamDatum::Screenshots(..) => None,
            UpstreamDatum::DebianITP(_c) => None,
            UpstreamDatum::Registry(_r) => None,
            UpstreamDatum::CiteAs(_c) => None,
            UpstreamDatum::Donation(_d) => None,
            UpstreamDatum::Webservice(w) => Some(w.parse().ok()?),
        }
    }

    pub fn as_person(&self) -> Option<&Person> {
        match self {
            UpstreamDatum::Maintainer(p) => Some(p),
            _ => None,
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
            UpstreamDatum::Registry(r) => {
                write!(f, "Registry:")?;
                for (k, v) in r {
                    write!(f, "  - Name: {}", k)?;
                    write!(f, "    Entry: {}", v)?;
                }
                Ok(())
            }
            UpstreamDatum::CiteAs(c) => {
                write!(f, "Cite-As: {}", c)
            }
            UpstreamDatum::Donation(d) => {
                write!(f, "Donation: {}", d)
            }
            UpstreamDatum::Webservice(w) => {
                write!(f, "Webservice: {}", w)
            }
        }
    }
}

impl serde::ser::Serialize for UpstreamDatum {
    fn serialize<S: serde::ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            UpstreamDatum::Name(s) => serializer.serialize_str(s),
            UpstreamDatum::Homepage(s) => serializer.serialize_str(s),
            UpstreamDatum::Repository(s) => serializer.serialize_str(s),
            UpstreamDatum::RepositoryBrowse(s) => serializer.serialize_str(s),
            UpstreamDatum::Description(s) => serializer.serialize_str(s),
            UpstreamDatum::Summary(s) => serializer.serialize_str(s),
            UpstreamDatum::License(s) => serializer.serialize_str(s),
            UpstreamDatum::BugDatabase(s) => serializer.serialize_str(s),
            UpstreamDatum::BugSubmit(s) => serializer.serialize_str(s),
            UpstreamDatum::Contact(s) => serializer.serialize_str(s),
            UpstreamDatum::CargoCrate(s) => serializer.serialize_str(s),
            UpstreamDatum::SecurityMD(s) => serializer.serialize_str(s),
            UpstreamDatum::SecurityContact(s) => serializer.serialize_str(s),
            UpstreamDatum::Version(s) => serializer.serialize_str(s),
            UpstreamDatum::Documentation(s) => serializer.serialize_str(s),
            UpstreamDatum::GoImportPath(s) => serializer.serialize_str(s),
            UpstreamDatum::Download(s) => serializer.serialize_str(s),
            UpstreamDatum::Wiki(s) => serializer.serialize_str(s),
            UpstreamDatum::MailingList(s) => serializer.serialize_str(s),
            UpstreamDatum::SourceForgeProject(s) => serializer.serialize_str(s),
            UpstreamDatum::Archive(s) => serializer.serialize_str(s),
            UpstreamDatum::Demo(s) => serializer.serialize_str(s),
            UpstreamDatum::PeclPackage(s) => serializer.serialize_str(s),
            UpstreamDatum::Author(authors) => {
                let mut seq = serializer.serialize_seq(Some(authors.len()))?;
                for a in authors {
                    seq.serialize_element(a)?;
                }
                seq.end()
            }
            UpstreamDatum::Maintainer(maintainer) => maintainer.serialize(serializer),
            UpstreamDatum::Keywords(keywords) => {
                let mut seq = serializer.serialize_seq(Some(keywords.len()))?;
                for a in keywords {
                    seq.serialize_element(a)?;
                }
                seq.end()
            }
            UpstreamDatum::Copyright(s) => serializer.serialize_str(s),
            UpstreamDatum::Funding(s) => serializer.serialize_str(s),
            UpstreamDatum::Changelog(s) => serializer.serialize_str(s),
            UpstreamDatum::DebianITP(s) => serializer.serialize_i32(*s),
            UpstreamDatum::HaskellPackage(p) => serializer.serialize_str(p),
            UpstreamDatum::Screenshots(s) => {
                let mut seq = serializer.serialize_seq(Some(s.len()))?;
                for s in s {
                    seq.serialize_element(s)?;
                }
                seq.end()
            }
            UpstreamDatum::CiteAs(c) => serializer.serialize_str(c),
            UpstreamDatum::Registry(r) => {
                let mut l = serializer.serialize_seq(Some(r.len()))?;
                for (k, v) in r {
                    let mut m = serde_yaml::Mapping::new();
                    m.insert(
                        serde_yaml::Value::String("Name".to_string()),
                        serde_yaml::to_value(k).unwrap(),
                    );
                    m.insert(
                        serde_yaml::Value::String("Entry".to_string()),
                        serde_yaml::to_value(v).unwrap(),
                    );
                    l.serialize_element(&m)?;
                }
                l.end()
            }
            UpstreamDatum::Donation(d) => serializer.serialize_str(d),
            UpstreamDatum::Webservice(w) => serializer.serialize_str(w),
        }
    }
}

pub struct UpstreamMetadata(Vec<UpstreamDatumWithMetadata>);

impl UpstreamMetadata {
    pub fn new() -> Self {
        UpstreamMetadata(Vec::new())
    }

    pub fn from_data(data: Vec<UpstreamDatumWithMetadata>) -> Self {
        Self(data)
    }

    pub fn mut_items(&mut self) -> &mut Vec<UpstreamDatumWithMetadata> {
        &mut self.0
    }

    pub fn iter(&self) -> impl Iterator<Item = &UpstreamDatumWithMetadata> {
        self.0.iter()
    }

    pub fn mut_iter(&mut self) -> impl Iterator<Item = &mut UpstreamDatumWithMetadata> {
        self.0.iter_mut()
    }

    pub fn get(&self, field: &str) -> Option<&UpstreamDatumWithMetadata> {
        self.0.iter().find(|d| d.datum.field() == field)
    }

    pub fn get_mut(&mut self, field: &str) -> Option<&mut UpstreamDatumWithMetadata> {
        self.0.iter_mut().find(|d| d.datum.field() == field)
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

    pub fn update(
        &mut self,
        new_items: impl Iterator<Item = UpstreamDatumWithMetadata>,
    ) -> Vec<UpstreamDatumWithMetadata> {
        update_from_guesses(&mut self.0, new_items)
    }

    pub fn remove(&mut self, field: &str) -> Option<UpstreamDatumWithMetadata> {
        let index = self.0.iter().position(|d| d.datum.field() == field)?;
        Some(self.0.remove(index))
    }
}

impl Default for UpstreamMetadata {
    fn default() -> Self {
        UpstreamMetadata::new()
    }
}

impl Iterator for UpstreamMetadata {
    type Item = UpstreamDatumWithMetadata;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.pop()
    }
}

impl From<Vec<UpstreamDatumWithMetadata>> for UpstreamMetadata {
    fn from(v: Vec<UpstreamDatumWithMetadata>) -> Self {
        UpstreamMetadata(v)
    }
}

impl From<Vec<UpstreamDatum>> for UpstreamMetadata {
    fn from(v: Vec<UpstreamDatum>) -> Self {
        UpstreamMetadata(
            v.into_iter()
                .map(|d| UpstreamDatumWithMetadata {
                    datum: d,
                    certainty: None,
                    origin: None,
                })
                .collect(),
        )
    }
}

impl From<UpstreamMetadata> for Vec<UpstreamDatumWithMetadata> {
    fn from(v: UpstreamMetadata) -> Self {
        v.0
    }
}

impl From<UpstreamMetadata> for Vec<UpstreamDatum> {
    fn from(v: UpstreamMetadata) -> Self {
        v.0.into_iter().map(|d| d.datum).collect()
    }
}

impl serde::ser::Serialize for UpstreamMetadata {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        let mut map = serde_yaml::Mapping::new();
        for datum in &self.0 {
            map.insert(
                serde_yaml::Value::String(datum.datum.field().to_string()),
                serde_yaml::to_value(datum).unwrap(),
            );
        }
        map.serialize(serializer)
    }
}

impl ToPyObject for UpstreamDatumWithMetadata {
    fn to_object(&self, py: Python) -> PyObject {
        let m = PyModule::import_bound(py, "upstream_ontologist.guess").unwrap();

        let cls = m.getattr("UpstreamDatum").unwrap();

        let (field, py_datum) = self
            .datum
            .to_object(py)
            .extract::<(String, PyObject)>(py)
            .unwrap();

        let kwargs = pyo3::types::PyDict::new_bound(py);
        kwargs
            .set_item("certainty", self.certainty.map(|x| x.to_string()))
            .unwrap();
        kwargs.set_item("origin", self.origin.as_ref()).unwrap();

        let datum = cls.call((field, py_datum), Some(&kwargs)).unwrap();

        datum.to_object(py)
    }
}

impl serde::ser::Serialize for UpstreamDatumWithMetadata {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        UpstreamDatum::serialize(&self.datum, serializer)
    }
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
    let mut headers = HeaderMap::new();
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

    let client = crate::http::build_client()
        .timeout(timeout)
        .default_headers(headers)
        .build()
        .map_err(HTTPJSONError::HTTPError)?;

    let http_url: reqwest::Url = Into::<String>::into(http_url.clone()).parse().unwrap();

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

    let client = crate::http::build_client()
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
        _max_certainty: Option<Certainty>,
    ) {
    }
}

pub struct GitHub;

impl Default for GitHub {
    fn default() -> Self {
        Self::new()
    }
}

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

impl Default for GitLab {
    fn default() -> Self {
        Self::new()
    }
}

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
    _settings: &GuesserSettings,
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

pub fn update_from_guesses(
    metadata: &mut Vec<UpstreamDatumWithMetadata>,
    new_items: impl Iterator<Item = UpstreamDatumWithMetadata>,
) -> Vec<UpstreamDatumWithMetadata> {
    let mut changed = vec![];
    for datum in new_items {
        let current_datum = find_datum(metadata, datum.datum.field());
        if current_datum.is_none() || datum.certainty > current_datum.unwrap().certainty {
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
    max_certainty: Option<Certainty>,
    supported_fields: &[&str],
    new_items: impl Fn() -> Vec<UpstreamDatum>,
) {
    if max_certainty.is_some()
        && !possible_fields_missing(metadata, supported_fields, max_certainty.unwrap())
    {
        return;
    }

    let new_items = new_items()
        .into_iter()
        .map(|item| UpstreamDatumWithMetadata {
            datum: item,
            certainty: max_certainty,
            origin: None,
        });

    update_from_guesses(metadata, new_items);
}

pub struct SourceForge;

impl Default for SourceForge {
    fn default() -> Self {
        Self::new()
    }
}

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
        max_certainty: Option<Certainty>,
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

impl Default for Launchpad {
    fn default() -> Self {
        Self::new()
    }
}

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

#[test]
fn test_bug_database_url_from_bug_submit_url() {
    let url = Url::parse("https://bugs.launchpad.net/bugs/+filebug").unwrap();
    assert_eq!(
        bug_database_url_from_bug_submit_url(&url, None).unwrap(),
        Url::parse("https://bugs.launchpad.net/bugs").unwrap()
    );

    let url = Url::parse("https://github.com/dulwich/dulwich/issues/new").unwrap();

    assert_eq!(
        bug_database_url_from_bug_submit_url(&url, None).unwrap(),
        Url::parse("https://github.com/dulwich/dulwich/issues").unwrap()
    );

    let url = Url::parse("https://sourceforge.net/p/dulwich/bugs/new").unwrap();

    assert_eq!(
        bug_database_url_from_bug_submit_url(&url, None).unwrap(),
        Url::parse("https://sourceforge.net/p/dulwich/bugs").unwrap()
    );
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
    use scraper::Html;

    let soup = Html::parse_document(page);

    let selector = scraper::Selector::parse("[id=access_url]").unwrap();

    let el = soup.select(&selector).next()?;

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
        Err(HTTPJSONError::Error { status: 404, .. }) => None,
        Err(e) => {
            debug!("Failed to load repology metadata: {:?}", e);
            None
        }
    }
}

pub fn guess_from_path(
    path: &Path,
    _settings: &GuesserSettings,
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
    fn extract_bound(obj: &Bound<PyAny>) -> PyResult<Self> {
        let (field, val): (String, Bound<PyAny>) = if let Ok((field, val)) =
            obj.extract::<(String, Bound<PyAny>)>()
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
            "Cite-As" => Ok(UpstreamDatum::CiteAs(val.extract::<String>()?)),
            "Registry" => {
                let v = val.extract::<Vec<Bound<PyAny>>>()?;
                let mut registry = Vec::new();
                for item in v {
                    let name = item.get_item("Name")?.extract::<String>()?;
                    let entry = item.get_item("Entry")?.extract::<String>()?;
                    registry.push((name, entry));
                }
                Ok(UpstreamDatum::Registry(registry))
            }
            "Donation" => Ok(UpstreamDatum::Donation(val.extract::<String>()?)),
            "Webservice" => Ok(UpstreamDatum::Webservice(val.extract::<String>()?)),
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
                UpstreamDatum::CiteAs(s) => s.to_object(py),
                UpstreamDatum::Registry(r) => r
                    .iter()
                    .map(|(name, entry)| {
                        let dict = PyDict::new_bound(py);
                        dict.set_item("Name", name).unwrap();
                        dict.set_item("Entry", entry).unwrap();
                        dict.into()
                    })
                    .collect::<Vec<PyObject>>()
                    .to_object(py),
                UpstreamDatum::Donation(d) => d.to_object(py),
                UpstreamDatum::Webservice(w) => w.to_object(py),
            },
        )
            .to_object(py)
    }
}

impl FromPyObject<'_> for UpstreamDatumWithMetadata {
    fn extract_bound(obj: &Bound<PyAny>) -> PyResult<Self> {
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
    ExtrapolationLimitExceeded(usize),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ProviderError::ParseError(e) => write!(f, "Parse error: {}", e),
            ProviderError::IoError(e) => write!(f, "IO error: {}", e),
            ProviderError::Other(e) => write!(f, "Other error: {}", e),
            ProviderError::HttpJsonError(e) => write!(f, "HTTP JSON error: {}", e),
            ProviderError::Python(e) => write!(f, "Python error: {}", e),
            ProviderError::ExtrapolationLimitExceeded(e) => {
                write!(f, "Extrapolation limit exceeded: {}", e)
            }
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

impl From<reqwest::Error> for ProviderError {
    fn from(e: reqwest::Error) -> Self {
        ProviderError::Other(e.to_string())
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
            ProviderError::ExtrapolationLimitExceeded(e) => {
                PyRuntimeError::new_err((e.to_string(),))
            }
        }
    }
}

#[derive(Debug)]
pub struct UpstreamPackage {
    pub family: String,
    pub name: String,
}

impl FromPyObject<'_> for UpstreamPackage {
    fn extract_bound(obj: &Bound<PyAny>) -> PyResult<Self> {
        let family = obj.getattr("family")?.extract::<String>()?;
        let name = obj.getattr("name")?.extract::<String>()?;
        Ok(UpstreamPackage { family, name })
    }
}

impl ToPyObject for UpstreamPackage {
    fn to_object(&self, py: Python) -> PyObject {
        let dict = pyo3::types::PyDict::new_bound(py);
        dict.set_item("family", self.family.clone()).unwrap();
        dict.set_item("name", self.name.clone()).unwrap();
        dict.into()
    }
}

#[derive(Debug)]
pub struct UpstreamVersion(String);

impl FromPyObject<'_> for UpstreamVersion {
    fn extract_bound(obj: &Bound<PyAny>) -> PyResult<Self> {
        let version = obj.extract::<String>()?;
        Ok(UpstreamVersion(version))
    }
}

impl ToPyObject for UpstreamVersion {
    fn to_object(&self, py: Python) -> PyObject {
        self.0.to_object(py)
    }
}

#[derive(Debug, Default)]
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
    fn(&std::path::Path, &GuesserSettings) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError>,
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
    (
        "metadata.json",
        crate::providers::metadata_json::guess_from_metadata_json,
    ),
    (".travis.yml", crate::guess_from_travis_yml),
];

fn find_guessers(path: &std::path::Path) -> Vec<Box<dyn Guesser>> {
    let mut candidates: Vec<Box<dyn Guesser>> = Vec::new();

    let path = path.canonicalize().unwrap();

    for (name, cb) in STATIC_GUESSERS {
        let subpath = path.join(name);
        if subpath.exists() {
            candidates.push(Box::new(PathGuesser {
                name: name.to_string(),
                subpath: subpath.clone(),
                cb: Box::new(cb),
            }));
        }
    }

    for name in ["SECURITY.md", ".github/SECURITY.md", "docs/SECURITY.md"].iter() {
        if path.join(name).exists() {
            let subpath = path.join(name);
            candidates.push(Box::new(PathGuesser {
                name: name.to_string(),
                subpath: subpath.clone(),
                cb: Box::new(|p, s| {
                    crate::providers::security_md::guess_from_security_md(name, p, s)
                }),
            }));
        }
    }

    let mut found_pkg_info = path.join("PKG-INFO").exists();
    for entry in std::fs::read_dir(&path).unwrap() {
        let entry = entry.unwrap();
        let filename = entry.file_name().to_string_lossy().to_string();
        if filename.ends_with(".egg-info") {
            candidates.push(Box::new(PathGuesser {
                name: format!("{}/PKG-INFO", filename),
                subpath: entry.path().join("PKG-INFO"),
                cb: Box::new(crate::providers::python::guess_from_pkg_info),
            }));
            found_pkg_info = true;
        } else if filename.ends_with(".dist-info") {
            candidates.push(Box::new(PathGuesser {
                name: format!("{}/METADATA", filename),
                subpath: entry.path().join("METADATA"),
                cb: Box::new(crate::providers::python::guess_from_pkg_info),
            }));
            found_pkg_info = true;
        }
    }

    if !found_pkg_info && path.join("setup.py").exists() {
        candidates.push(Box::new(PathGuesser {
            name: "setup.py".to_string(),
            subpath: path.join("setup.py"),
            cb: Box::new(|path, s| {
                crate::providers::python::guess_from_setup_py(path, s.trust_package)
            }),
        }));
    }

    for entry in std::fs::read_dir(&path).unwrap() {
        let entry = entry.unwrap();

        if entry.file_name().to_string_lossy().ends_with(".gemspec") {
            candidates.push(Box::new(PathGuesser {
                name: entry.file_name().to_string_lossy().to_string(),
                subpath: entry.path(),
                cb: Box::new(crate::providers::ruby::guess_from_gemspec),
            }));
        }
    }

    // TODO(jelmer): Perhaps scan all directories if no other primary project information file has been found?
    for entry in std::fs::read_dir(&path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if entry.file_type().unwrap().is_dir() {
            let description_name = format!("{}/DESCRIPTION", entry.file_name().to_string_lossy());
            if path.join(&description_name).exists() {
                candidates.push(Box::new(PathGuesser {
                    name: description_name,
                    subpath: path.join("DESCRIPTION"),
                    cb: Box::new(crate::providers::r::guess_from_r_description),
                }));
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
        candidates.push(Box::new(PathGuesser {
            name: doap_filename.to_string_lossy().to_string(),
            subpath: path.join(&doap_filename),
            cb: Box::new(|p, s| crate::providers::doap::guess_from_doap(p, s.trust_package)),
        }));
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
        candidates.push(Box::new(PathGuesser {
            name: metainfo_filename.to_string_lossy().to_string(),
            subpath: path.join(&metainfo_filename),
            cb: Box::new(|p, s| {
                crate::providers::metainfo::guess_from_metainfo(p, s.trust_package)
            }),
        }));
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
        candidates.push(Box::new(PathGuesser {
            name: cabal_filename.to_string_lossy().to_string(),
            subpath: path.join(&cabal_filename),
            cb: Box::new(|path, s| {
                crate::providers::haskell::guess_from_cabal(path, s.trust_package)
            }),
        }));
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
        candidates.push(Box::new(PathGuesser {
            name: filename.to_string_lossy().to_string(),
            subpath: path.join(&filename),
            cb: Box::new(|path, s| crate::readme::guess_from_readme(path, s.trust_package)),
        }));
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
        candidates.push(Box::new(PathGuesser {
            name: nuspec_filename.to_string_lossy().to_string(),
            subpath: path.join(&nuspec_filename),
            cb: Box::new(|path, s| {
                crate::providers::nuspec::guess_from_nuspec(path, s.trust_package)
            }),
        }));
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

    match opam_filenames.len().cmp(&1) {
        Ordering::Equal => {
            let opam_filename = opam_filenames.remove(0);
            candidates.push(Box::new(PathGuesser {
                name: opam_filename.to_string_lossy().to_string(),
                subpath: path.join(&opam_filename),
                cb: Box::new(|path, s| {
                    crate::providers::ocaml::guess_from_opam(path, s.trust_package)
                }),
            }));
        }
        Ordering::Greater => {
            log::warn!(
                "Multiple opam files found: {:?}, ignoring all.",
                opam_filenames
            );
        }
        Ordering::Less => {}
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
        candidates.push(Box::new(PathGuesser {
            name: filename.clone(),
            subpath: path.join(&filename),
            cb: Box::new(crate::providers::debian::guess_from_debian_patch),
        }));
    }

    candidates.push(Box::new(EnvironmentGuesser::new()));
    candidates.push(Box::new(PathGuesser {
        name: ".".to_string(),
        subpath: path.clone(),
        cb: Box::new(crate::guess_from_path),
    }));

    candidates
}

pub struct UpstreamMetadataScanner {
    path: std::path::PathBuf,
    config: GuesserSettings,
    pending: Vec<UpstreamDatumWithMetadata>,
    guessers: Vec<Box<dyn Guesser>>,
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

            let mut guesser = self.guessers.remove(0);

            let abspath = std::env::current_dir().unwrap().join(self.path.as_path());

            let guess = guesser.guess(&self.config);
            match guess {
                Ok(entries) => {
                    self.pending.extend(entries.into_iter().map(|mut e| {
                        log::trace!("{}: {:?}", guesser.name(), e);
                        e.origin = e.origin.or(Some(Origin::Other(guesser.name().to_string())));
                        if let Some(Origin::Path(p)) = e.origin.as_ref() {
                            if let Ok(suffix) = p.strip_prefix(abspath.as_path()) {
                                if suffix.to_str().unwrap().is_empty() {
                                    e.origin = Some(Origin::Path(PathBuf::from_str(".").unwrap()));
                                } else {
                                    e.origin = Some(Origin::Path(
                                        PathBuf::from_str(".").unwrap().join(suffix),
                                    ));
                                }
                            }
                        }
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

pub fn extend_upstream_metadata(
    upstream_metadata: &mut UpstreamMetadata,
    path: &std::path::Path,
    minimum_certainty: Option<Certainty>,
    net_access: Option<bool>,
    consult_external_directory: Option<bool>,
) -> Result<(), ProviderError> {
    let net_access = net_access.unwrap_or(false);
    let consult_external_directory = consult_external_directory.unwrap_or(false);
    let minimum_certainty = minimum_certainty.unwrap_or(Certainty::Confident);

    // TODO(jelmer): Use EXTRAPOLATE_FNS mechanism for this?
    for field in [
        "Homepage",
        "Bug-Database",
        "Bug-Submit",
        "Repository",
        "Repository-Browse",
        "Download",
    ] {
        let value = match upstream_metadata.get(field) {
            Some(value) => value,
            None => continue,
        };

        if let Some(project) = extract_sf_project_name(value.datum.as_str().unwrap()) {
            let certainty = Some(
                std::cmp::min(Some(Certainty::Likely), value.certainty)
                    .unwrap_or(Certainty::Likely),
            );
            upstream_metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Archive("SourceForge".to_string()),
                certainty,
                origin: Some(Origin::Other(format!("derived from {}", field))),
            });
            upstream_metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::SourceForgeProject(project),
                certainty,
                origin: Some(Origin::Other(format!("derived from {}", field))),
            });
            break;
        }
    }

    let archive = upstream_metadata.get("Archive");
    if archive.is_some()
        && archive.unwrap().datum.as_str().unwrap() == "SourceForge"
        && upstream_metadata.contains_key("SourceForge-Project")
        && net_access
    {
        let sf_project = upstream_metadata
            .get("SourceForge-Project")
            .unwrap()
            .datum
            .as_str()
            .unwrap()
            .to_string();
        let sf_certainty = archive.unwrap().certainty;
        SourceForge::new().extend_metadata(
            upstream_metadata.mut_items(),
            sf_project.as_str(),
            sf_certainty,
        );
    }

    let archive = upstream_metadata.get("Archive");
    if archive.is_some()
        && archive.unwrap().datum.as_str().unwrap() == "Hackage"
        && upstream_metadata.contains_key("Hackage-Package")
        && net_access
    {
        let hackage_package = upstream_metadata
            .get("Hackage-Package")
            .unwrap()
            .datum
            .as_str()
            .unwrap()
            .to_string();
        let hackage_certainty = archive.unwrap().certainty;

        crate::providers::haskell::Hackage::new()
            .extend_metadata(
                upstream_metadata.mut_items(),
                hackage_package.as_str(),
                hackage_certainty,
            )
            .unwrap();
    }

    let archive = upstream_metadata.get("Archive");
    if archive.is_some()
        && archive.unwrap().datum.as_str().unwrap() == "crates.io"
        && upstream_metadata.contains_key("Cargo-Crate")
        && net_access
    {
        let cargo_crate = upstream_metadata
            .get("Cargo-Crate")
            .unwrap()
            .datum
            .as_str()
            .unwrap()
            .to_string();
        let crates_io_certainty = upstream_metadata.get("Archive").unwrap().certainty;
        crate::providers::rust::CratesIo::new()
            .extend_metadata(
                upstream_metadata.mut_items(),
                cargo_crate.as_str(),
                crates_io_certainty,
            )
            .unwrap();
    }

    let archive = upstream_metadata.get("Archive");
    if archive.is_some()
        && archive.unwrap().datum.as_str().unwrap() == "Pecl"
        && upstream_metadata.contains_key("Pecl-Package")
        && net_access
    {
        let pecl_package = upstream_metadata
            .get("Pecl-Package")
            .unwrap()
            .datum
            .as_str()
            .unwrap()
            .to_string();
        let pecl_certainty = upstream_metadata.get("Archive").unwrap().certainty;
        crate::providers::php::Pecl::new()
            .extend_metadata(
                upstream_metadata.mut_items(),
                pecl_package.as_str(),
                pecl_certainty,
            )
            .unwrap();
    }

    if net_access && consult_external_directory {
        // TODO(jelmer): Don't assume debian/control exists
        let package = match debian_control::Control::from_file_relaxed(path.join("debian/control"))
        {
            Ok((control, _)) => control.source().and_then(|s| s.get("Package")),
            Err(_) => None,
        };

        if let Some(package) = package {
            extend_from_lp(
                upstream_metadata.mut_items(),
                minimum_certainty,
                package.as_str(),
                None,
                None,
            );
            crate::providers::arch::Aur::new()
                .extend_metadata(
                    upstream_metadata.mut_items(),
                    package.as_str(),
                    Some(minimum_certainty),
                )
                .unwrap();
            crate::providers::gobo::Gobo::new()
                .extend_metadata(
                    upstream_metadata.mut_items(),
                    package.as_str(),
                    Some(minimum_certainty),
                )
                .unwrap();
            extend_from_repology(
                upstream_metadata.mut_items(),
                minimum_certainty,
                package.as_str(),
            );
        }
    }
    crate::extrapolate::extrapolate_fields(upstream_metadata, net_access, None)?;
    Ok(())
}

pub trait ThirdPartyRepository {
    fn name(&self) -> &'static str;
    fn supported_fields(&self) -> &'static [&'static str];
    fn max_supported_certainty(&self) -> Certainty;

    fn extend_metadata(
        &self,
        metadata: &mut Vec<UpstreamDatumWithMetadata>,
        name: &str,
        min_certainty: Option<Certainty>,
    ) -> Result<(), ProviderError> {
        if min_certainty.is_some() && min_certainty.unwrap() > self.max_supported_certainty() {
            // Don't bother if we can't meet minimum certainty
            return Ok(());
        }

        extend_from_external_guesser(
            metadata,
            Some(self.max_supported_certainty()),
            self.supported_fields(),
            || self.guess_metadata(name).unwrap(),
        );

        Ok(())
    }

    fn guess_metadata(&self, name: &str) -> Result<Vec<UpstreamDatum>, ProviderError>;
}

fn extend_from_lp(
    upstream_metadata: &mut Vec<UpstreamDatumWithMetadata>,
    minimum_certainty: Certainty,
    package: &str,
    distribution: Option<&str>,
    suite: Option<&str>,
) {
    // The set of fields that Launchpad can possibly provide:
    let lp_fields = &["Homepage", "Repository", "Name", "Download"][..];
    let lp_certainty = Certainty::Possible;

    if lp_certainty < minimum_certainty {
        // Don't bother talking to launchpad if we're not
        // speculating.
        return;
    }

    extend_from_external_guesser(upstream_metadata, Some(lp_certainty), lp_fields, || {
        crate::providers::launchpad::guess_from_launchpad(package, distribution, suite).unwrap()
    })
}

fn extend_from_repology(
    upstream_metadata: &mut Vec<UpstreamDatumWithMetadata>,
    minimum_certainty: Certainty,
    source_package: &str,
) {
    // The set of fields that repology can possibly provide:
    let repology_fields = &["Homepage", "License", "Summary", "Download"][..];
    let certainty = Certainty::Confident;

    if certainty < minimum_certainty {
        // Don't bother talking to repology if we're not speculating.
        return;
    }

    extend_from_external_guesser(upstream_metadata, Some(certainty), repology_fields, || {
        crate::providers::repology::guess_from_repology(source_package).unwrap()
    })
}

/// Fix existing upstream metadata.
pub fn fix_upstream_metadata(upstream_metadata: &mut UpstreamMetadata) {
    if let Some(repository) = upstream_metadata.get_mut("Repository") {
        let url = crate::vcs::sanitize_url(repository.datum.as_str().unwrap());
        repository.datum = UpstreamDatum::Repository(url.to_string());
    }

    if let Some(summary) = upstream_metadata.get_mut("Summary") {
        let s = summary.datum.as_str().unwrap();
        let s = s.split_once(". ").map_or(s, |(a, _)| a);
        let s = s.trim_end().trim_end_matches('.');
        summary.datum = UpstreamDatum::Summary(s.to_string());
    }
}

/// Summarize the upstream metadata into a dictionary.
///
/// # Arguments
/// * `metadata_items`: Iterator over metadata items
/// * `path`: Path to the package
/// * `trust_package`: Whether to trust the package contents and i.e. run executables in it
/// * `net_access`: Whether to allow net access
/// * `consult_external_directory`: Whether to pull in data from external (user-maintained) directories.
pub fn summarize_upstream_metadata(
    metadata_items: impl Iterator<Item = UpstreamDatumWithMetadata>,
    path: &std::path::Path,
    net_access: Option<bool>,
    consult_external_directory: Option<bool>,
    check: Option<bool>,
) -> Result<UpstreamMetadata, ProviderError> {
    let check = check.unwrap_or(false);
    let mut upstream_metadata = UpstreamMetadata::new();
    upstream_metadata.update(filter_bad_guesses(metadata_items));

    extend_upstream_metadata(
        &mut upstream_metadata,
        path,
        None,
        net_access,
        consult_external_directory,
    )?;

    if check {
        check_upstream_metadata(&mut upstream_metadata, None);
    }

    fix_upstream_metadata(&mut upstream_metadata);

    Ok(upstream_metadata)
}

/// Guess upstream metadata items, in no particular order.
///
/// # Arguments
/// * `path`: Path to the package
/// * `trust_package`: Whether to trust the package contents and i.e. run executables in it
/// * `minimum_certainty`: Minimum certainty of guesses to return
pub fn guess_upstream_metadata_items(
    path: &std::path::Path,
    trust_package: Option<bool>,
    minimum_certainty: Option<Certainty>,
) -> impl Iterator<Item = Result<UpstreamDatumWithMetadata, ProviderError>> {
    guess_upstream_info(path, trust_package).filter_map(move |e| match e {
        Err(e) => Some(Err(e)),
        Ok(UpstreamDatumWithMetadata {
            datum,
            certainty,
            origin,
        }) => {
            if minimum_certainty.is_some() && certainty < minimum_certainty {
                None
            } else {
                Some(Ok(UpstreamDatumWithMetadata {
                    datum,
                    certainty,
                    origin,
                }))
            }
        }
    })
}

pub fn get_upstream_info(
    path: &std::path::Path,
    trust_package: Option<bool>,
    net_access: Option<bool>,
    consult_external_directory: Option<bool>,
    check: Option<bool>,
) -> Result<UpstreamMetadata, ProviderError> {
    let metadata_items = guess_upstream_info(path, trust_package);
    summarize_upstream_metadata(
        metadata_items.filter_map(|x| match x {
            Ok(x) => Some(x),
            Err(e) => {
                log::error!("{}", e);
                None
            }
        }),
        path,
        net_access,
        consult_external_directory,
        check,
    )
}

/// Guess the upstream metadata dictionary.
///
/// # Arguments
/// * `path`: Path to the package
/// * `trust_package`: Whether to trust the package contents and i.e. run executables in it
/// * `net_access`: Whether to allow net access
/// * `consult_external_directory`: Whether to pull in data from external (user-maintained) directories.
pub fn guess_upstream_metadata(
    path: &std::path::Path,
    trust_package: Option<bool>,
    net_access: Option<bool>,
    consult_external_directory: Option<bool>,
    check: Option<bool>,
) -> Result<UpstreamMetadata, ProviderError> {
    let metadata_items =
        guess_upstream_metadata_items(path, trust_package, None).filter_map(|x| match x {
            Ok(x) => Some(x),
            Err(e) => {
                log::error!("{}", e);
                None
            }
        });
    summarize_upstream_metadata(
        metadata_items,
        path,
        net_access,
        consult_external_directory,
        check,
    )
}

pub fn verify_screenshots(urls: &[&str]) -> Vec<(String, Option<bool>)> {
    let mut ret = Vec::new();
    for url in urls {
        let mut request =
            reqwest::blocking::Request::new(reqwest::Method::GET, url.parse().unwrap());
        request.headers_mut().insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static(USER_AGENT),
        );

        match reqwest::blocking::Client::new().execute(request) {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    ret.push((url.to_string(), Some(true)));
                } else if status.is_client_error() {
                    ret.push((url.to_string(), Some(false)));
                } else {
                    ret.push((url.to_string(), None));
                }
            }
            Err(e) => {
                log::debug!("Error fetching {}: {}", url, e);
                ret.push((url.to_string(), None));
            }
        }
    }

    ret
}

/// Check upstream metadata.
///
/// This will make network connections, etc.
pub fn check_upstream_metadata(upstream_metadata: &mut UpstreamMetadata, version: Option<&str>) {
    let repository = upstream_metadata.get_mut("Repository");
    if let Some(repository) = repository {
        match vcs::check_repository_url_canonical(repository.datum.to_url().unwrap(), version) {
            Ok(canonical_url) => {
                repository.datum = UpstreamDatum::Repository(canonical_url.to_string());
                if repository.certainty == Some(Certainty::Confident) {
                    repository.certainty = Some(Certainty::Certain);
                }
                let derived_browse_url = vcs::browse_url_from_repo_url(
                    &vcs::VcsLocation {
                        url: repository.datum.to_url().unwrap(),
                        branch: None,
                        subpath: None,
                    },
                    Some(true),
                );
                let certainty = repository.certainty;
                let browse_repo = upstream_metadata.get_mut("Repository-Browse");
                if browse_repo.is_some()
                    && derived_browse_url == browse_repo.as_ref().and_then(|u| u.datum.to_url())
                {
                    browse_repo.unwrap().certainty = certainty;
                }
            }
            Err(CanonicalizeError::Unverifiable(u, _)) | Err(CanonicalizeError::RateLimited(u)) => {
                log::debug!("Unverifiable URL: {}", u);
            }
            Err(CanonicalizeError::InvalidUrl(u, e)) => {
                log::debug!("Deleting invalid Repository URL {}: {}", u, e);
                upstream_metadata.remove("Repository");
            }
        }
    }
    let homepage = upstream_metadata.get_mut("Homepage");
    if let Some(homepage) = homepage {
        match check_url_canonical(&homepage.datum.to_url().unwrap()) {
            Ok(canonical_url) => {
                homepage.datum = UpstreamDatum::Homepage(canonical_url.to_string());
                if homepage.certainty >= Some(Certainty::Likely) {
                    homepage.certainty = Some(Certainty::Certain);
                }
            }
            Err(CanonicalizeError::Unverifiable(u, _)) | Err(CanonicalizeError::RateLimited(u)) => {
                log::debug!("Unverifiable URL: {}", u);
            }
            Err(CanonicalizeError::InvalidUrl(u, e)) => {
                log::debug!("Deleting invalid Homepage URL {}: {}", u, e);
                upstream_metadata.remove("Homepage");
            }
        }
    }
    if let Some(repository_browse) = upstream_metadata.get_mut("Repository-Browse") {
        match check_url_canonical(&repository_browse.datum.to_url().unwrap()) {
            Ok(u) => {
                repository_browse.datum = UpstreamDatum::RepositoryBrowse(u.to_string());
                if repository_browse.certainty >= Some(Certainty::Likely) {
                    repository_browse.certainty = Some(Certainty::Certain);
                }
            }
            Err(CanonicalizeError::InvalidUrl(u, e)) => {
                log::debug!("Deleting invalid Repository-Browse URL {}: {}", u, e);
                upstream_metadata.remove("Repository-Browse");
            }
            Err(CanonicalizeError::Unverifiable(u, _)) | Err(CanonicalizeError::RateLimited(u)) => {
                log::debug!("Unable to verify Repository-Browse URL {}", u);
            }
        }
    }
    if let Some(bug_database) = upstream_metadata.get_mut("Bug-Database") {
        match check_bug_database_canonical(&bug_database.datum.to_url().unwrap(), Some(true)) {
            Ok(u) => {
                bug_database.datum = UpstreamDatum::BugDatabase(u.to_string());
                if bug_database.certainty >= Some(Certainty::Likely) {
                    bug_database.certainty = Some(Certainty::Certain);
                }
            }
            Err(CanonicalizeError::InvalidUrl(u, e)) => {
                log::debug!("Deleting invalid Bug-Database URL {}: {}", u, e);
                upstream_metadata.remove("Bug-Database");
            }
            Err(CanonicalizeError::Unverifiable(u, _)) | Err(CanonicalizeError::RateLimited(u)) => {
                log::debug!("Unable to verify Bug-Database URL {}", u);
            }
        }
    }
    let bug_submit = upstream_metadata.get_mut("Bug-Submit");
    if let Some(bug_submit) = bug_submit {
        match check_bug_submit_url_canonical(&bug_submit.datum.to_url().unwrap(), Some(true)) {
            Ok(u) => {
                bug_submit.datum = UpstreamDatum::BugSubmit(u.to_string());
                if bug_submit.certainty >= Some(Certainty::Likely) {
                    bug_submit.certainty = Some(Certainty::Certain);
                }
            }
            Err(CanonicalizeError::InvalidUrl(u, e)) => {
                log::debug!("Deleting invalid Bug-Submit URL {}: {}", u, e);
                upstream_metadata.remove("Bug-Submit");
            }
            Err(CanonicalizeError::Unverifiable(u, _)) | Err(CanonicalizeError::RateLimited(u)) => {
                log::debug!("Unable to verify Bug-Submit URL {}", u);
            }
        }
    }
    let mut screenshots = upstream_metadata.get_mut("Screenshots");
    if screenshots.is_some() && screenshots.as_ref().unwrap().certainty == Some(Certainty::Likely) {
        let mut newvalue = vec![];
        screenshots.as_mut().unwrap().certainty = Some(Certainty::Certain);
        let urls = match &screenshots.as_ref().unwrap().datum {
            UpstreamDatum::Screenshots(urls) => urls,
            _ => unreachable!(),
        };
        for (url, status) in verify_screenshots(
            urls.iter()
                .map(|x| x.as_str())
                .collect::<Vec<&str>>()
                .as_slice(),
        ) {
            match status {
                Some(true) => {
                    newvalue.push(url);
                }
                Some(false) => {}
                None => {
                    screenshots.as_mut().unwrap().certainty = Some(Certainty::Likely);
                }
            }
        }
        screenshots.as_mut().unwrap().datum = UpstreamDatum::Screenshots(newvalue);
    }
}

pub fn filter_bad_guesses(
    guessed_items: impl Iterator<Item = UpstreamDatumWithMetadata>,
) -> impl Iterator<Item = UpstreamDatumWithMetadata> {
    guessed_items.filter(|item| {
        let bad = item.datum.known_bad_guess();
        if bad {
            log::debug!("Excluding known bad item {:?}", item);
        }
        !bad
    })
}

pub(crate) trait Guesser {
    fn name(&self) -> &str;

    fn guess(
        &mut self,
        settings: &GuesserSettings,
    ) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError>;
}

pub struct PathGuesser {
    name: String,
    subpath: std::path::PathBuf,
    cb: Box<
        dyn FnMut(
            &std::path::Path,
            &GuesserSettings,
        ) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError>,
    >,
}

impl Guesser for PathGuesser {
    fn name(&self) -> &str {
        &self.name
    }

    fn guess(
        &mut self,
        settings: &GuesserSettings,
    ) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
        (self.cb)(&self.subpath, settings)
    }
}

pub struct EnvironmentGuesser;

impl EnvironmentGuesser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EnvironmentGuesser {
    fn default() -> Self {
        Self::new()
    }
}

impl Guesser for EnvironmentGuesser {
    fn name(&self) -> &str {
        "environment"
    }

    fn guess(
        &mut self,
        _settings: &GuesserSettings,
    ) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
        crate::guess_from_environment()
    }
}
