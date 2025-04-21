// pyo3 macros use a gil-refs feature
#![allow(unexpected_cfgs)]
#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))]

use futures::stream::StreamExt;
use futures::Stream;
use lazy_regex::regex;
use log::{debug, warn};
use percent_encoding::utf8_percent_encode;
#[cfg(feature = "pyo3")]
use pyo3::{
    exceptions::{PyRuntimeError, PyTypeError, PyValueError},
    prelude::*,
    types::PyDict,
};
use reqwest::header::HeaderMap;
use serde::ser::SerializeSeq;
use std::cmp::Ordering;
use std::pin::Pin;
use std::str::FromStr;

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use url::Url;

static USER_AGENT: &str = concat!("upstream-ontologist/", env!("CARGO_PKG_VERSION"));

pub mod extrapolate;
pub mod forges;
pub mod homepage;
pub mod http;
pub mod providers;
pub mod readme;
pub mod repology;
pub mod vcs;
pub mod vcs_command;

#[cfg(test)]
mod upstream_tests {
    include!(concat!(env!("OUT_DIR"), "/upstream_tests.rs"));
}

#[cfg(test)]
mod readme_tests {
    include!(concat!(env!("OUT_DIR"), "/readme_tests.rs"));
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
/// Certainty levels for the data
pub enum Certainty {
    /// This datum is possibly correct, but it is a guess
    Possible,

    /// This datum is likely to be correct, but we are not sure
    Likely,

    /// We're confident about this datum, but there is a chance it is wrong
    Confident,

    /// We're certain about this datum
    Certain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Origin of the data
pub enum Origin {
    /// Read from a file
    Path(PathBuf),

    /// Read from a URL
    Url(url::Url),

    /// Other origin; described by a string
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

#[cfg(feature = "pyo3")]
impl ToPyObject for Origin {
    fn to_object(&self, py: Python) -> PyObject {
        match self {
            Origin::Path(path) => path.to_str().unwrap().to_object(py),
            Origin::Url(url) => url.to_string().to_object(py),
            Origin::Other(s) => s.to_object(py),
        }
    }
}

#[cfg(feature = "pyo3")]
impl IntoPy<PyObject> for Origin {
    fn into_py(self, py: Python) -> PyObject {
        match self {
            Origin::Path(path) => path.to_str().unwrap().to_object(py),
            Origin::Url(url) => url.to_string().to_object(py),
            Origin::Other(s) => s.to_object(py),
        }
    }
}

#[cfg(feature = "pyo3")]
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
        let o: String = ob.extract::<String>()?;
        o.parse().map_err(PyValueError::new_err)
    }
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub name: Option<String>,
    pub email: Option<String>,
    pub url: Option<String>,
}

impl serde::ser::Serialize for Person {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        let mut map = serde_yaml::Mapping::new();
        if let Some(name) = &self.name {
            map.insert(
                serde_yaml::Value::String("name".to_string()),
                serde_yaml::Value::String(name.to_string()),
            );
        }
        if let Some(email) = &self.email {
            map.insert(
                serde_yaml::Value::String("email".to_string()),
                serde_yaml::Value::String(email.to_string()),
            );
        }
        if let Some(url) = &self.url {
            map.insert(
                serde_yaml::Value::String("url".to_string()),
                serde_yaml::Value::String(url.to_string()),
            );
        }
        let tag = serde_yaml::value::TaggedValue {
            tag: serde_yaml::value::Tag::new("!Person"),
            value: serde_yaml::Value::Mapping(map),
        };
        tag.serialize(serializer)
    }
}

impl<'a> serde::de::Deserialize<'a> for Person {
    fn deserialize<D>(deserializer: D) -> Result<Person, D::Error>
    where
        D: serde::de::Deserializer<'a>,
    {
        let value = serde_yaml::Value::deserialize(deserializer)?;
        if let serde_yaml::Value::Mapping(map) = value {
            let mut name = None;
            let mut email = None;
            let mut url = None;
            for (k, v) in map {
                match k {
                    serde_yaml::Value::String(k) => match k.as_str() {
                        "name" => {
                            if let serde_yaml::Value::String(s) = v {
                                name = Some(s);
                            }
                        }
                        "email" => {
                            if let serde_yaml::Value::String(s) = v {
                                email = Some(s);
                            }
                        }
                        "url" => {
                            if let serde_yaml::Value::String(s) = v {
                                url = Some(s);
                            }
                        }
                        n => {
                            return Err(serde::de::Error::custom(format!("unknown key: {}", n)));
                        }
                    },
                    n => {
                        return Err(serde::de::Error::custom(format!(
                            "expected string key, got {:?}",
                            n
                        )));
                    }
                }
            }
            Ok(Person { name, email, url })
        } else {
            Err(serde::de::Error::custom("expected mapping"))
        }
    }
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

#[cfg(feature = "pyo3")]
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

#[cfg(feature = "pyo3")]
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
    /// Name of the project.
    ///
    /// This is a brief name of the project, as it would be used in a URL.
    /// Generally speaking it would be lowercase, and may contain dashes or underscores.
    /// It would commonly be the name of the repository.
    Name(String),

    /// URL to project homepage.
    ///
    /// This is the URL to the project's homepage, which may be a website or a
    /// repository. It is not a URL to a specific file or page, but rather the main
    /// entry point for the project.
    Homepage(String),

    /// URL to the project's source code repository.
    ///
    /// This is the URL to the project's source code repository, as it would be used
    /// in a command line tool to clone the repository. It may be a URL to a specific
    /// branch or tag, but it is generally the URL to the main repository.
    Repository(String),

    /// URL to browse the project's source code repository
    ///
    /// This is the URL to the project's source code repository, as it would be used
    /// in a web browser to browse the repository. It may be a URL to a specific
    /// branch or tag, but it is generally the URL to the main repository.
    RepositoryBrowse(String),

    /// Long description of the project
    ///
    /// This is a long description of the project, which may be several paragraphs
    /// long. It is generally a more detailed description of the project than the
    /// summary.
    Description(String),

    /// Short summary of the project (one line)
    ///
    /// This is a short summary of the project, which is generally one line long.
    /// It is generally a brief description of the project, and may be used in
    /// search results or in a list of projects.
    Summary(String),

    /// License name or SPDX identifier
    ///
    /// This is the name of the license under which the project is released. It may
    /// be a full license name, or it may be an SPDX identifier (preferred).
    ///
    /// See <https://spdx.org/licenses/> for a list of SPDX identifiers.
    License(String),

    /// List of authors
    ///
    /// This is a list of authors of the project, which may be a list of names,
    /// email addresses, or URLs.
    Author(Vec<Person>),

    /// List of maintainers
    ///
    /// This is a list of maintainers of the project, which may be a list of names,
    /// email addresses, or URLs.
    Maintainer(Person),

    /// URL of the project's issue tracker
    ///
    /// This is the URL to the project's issue tracker, which may be a bug tracker,
    /// feature tracker, or other type of issue tracker. It is not a URL to a
    /// specific issue, but rather the main entry point for the issue tracker.
    BugDatabase(String),

    /// URL to submit a new bug
    ///
    /// This is the URL to submit a new bug to the project's issue tracker. It
    /// may be a URL to a specific page or form.
    ///
    /// It can also be an email address (mailto:...), in which case it is the email address to send
    /// the bug report to.
    BugSubmit(String),

    /// URL to the project's contact page or email address
    ///
    /// This is the URL to the project's contact page, which may be a web page or
    /// an email address. It is not a URL to a specific file or page, but rather
    /// the main entry point for the contact page.
    Contact(String),

    /// Cargo crate name
    ///
    /// If the project is a Rust crate, this is the name of the crate on
    /// crates.io. It is not a URL to the crate, but rather the name of the
    /// crate.
    CargoCrate(String),

    /// Name of the security page name
    ///
    /// This would be the name of a markdown file in the source directory
    /// that contains security information about the project. It is not a URL to
    /// a specific file or page, but rather the name of the file.
    SecurityMD(String),

    /// URL to the security page or email address
    ///
    /// This is the URL to the project's security page, which may be a web page or
    /// an email address. It is not a URL to a specific file or page, but rather
    /// the main entry point for the security page.
    ///
    /// It can also be an email address (mailto:...), in which case it is the email address to send
    /// the security report to.
    SecurityContact(String),

    /// Last version of the project
    ///
    /// This is the last version of the project, which would generally be a version string
    ///
    /// There is no guarantee that this is the last version of the project.
    ///
    /// There is no guarantee about which versioning scheme is used, e.g. it may be
    /// a semantic version, a date-based version, or a commit hash.
    Version(String),

    /// List of keywords
    ///
    /// This is a list of keywords that describe the project. It may be a list of
    /// words, phrases, or tags.
    Keywords(Vec<String>),

    /// Copyright notice
    ///
    /// This is the copyright notice for the project, which may be a list of
    /// copyright holders, years, or other information.
    Copyright(String),

    /// URL to the project's documentation
    ///
    /// This is the URL to the project's documentation, which may be a web page or
    /// a file. It is not a URL to a specific file or page, but rather the main
    /// entry point for the documentation.
    Documentation(String),

    /// URL to the project's API documentation
    ///
    /// This is the URL to the project's API documentation, which may be a web page or
    /// a file. It is not a URL to a specific file or page, but rather the main
    /// entry point for the API documentation.
    APIDocumentation(String),

    /// Go import path
    ///
    /// If this is a Go project, this is the import path for the project. It is not a URL
    /// to the project, but rather the import path.
    GoImportPath(String),

    /// URL to the project's download page
    ///
    /// This is the URL to the project's download page, which may be a web page or
    /// a file. It is not a URL to a specific file or page, but rather the main
    /// entry point for the download page.
    Download(String),

    /// URL to the project's wiki
    ///
    /// This is the URL to the project's wiki.
    Wiki(String),

    /// URL to the project's mailing list
    ///
    /// This is the URL to the project's mailing list, which may be a web page or
    /// an email address. It is not a URL to a specific file or page, but rather
    /// the main entry point for the mailing list.
    ///
    /// It can also be an email address (mailto:...), in which case it is the email address to send
    /// email to to subscribe to the mailing list.
    MailingList(String),

    /// SourceForge project name
    ///
    /// This is the name of the project on SourceForge. It is not a URL to the
    /// project, but rather the name of the project.
    SourceForgeProject(String),

    /// If this project is provided by a specific archive, this is the name of the archive.
    ///
    /// E.g. "CRAN", "CPAN", "PyPI", "RubyGems", "NPM", etc.
    Archive(String),

    /// URL to a demo instance
    ///
    /// This is the URL to a demo instance of the project. This instance will be loaded
    /// with sample data, and will be used to demonstrate the project. It is not
    /// a full instance of the project - the Webservice field should be used for that.
    Demo(String),

    /// PHP PECL package name
    ///
    /// If this is a PHP project, this is the name of the package on PECL. It is not a URL
    /// to the package, but rather the name of the package.
    PeclPackage(String),

    /// Description of funding sources
    ///
    /// This is a description of the funding sources for the project. It may be a
    /// URL to a page that describes the funding sources, or it may be a list of
    /// funding sources.
    ///
    /// Note that this is different from the Donation field, which is a URL to a
    /// donation page.
    Funding(String),

    /// URL to the changelog
    ///
    /// This is the URL to the project's changelog, which may be a web page or
    /// a file. No guarantee is made about the format of the changelog, but it is
    /// generally a file that contains a list of changes made to the project.
    Changelog(String),

    /// Haskell package name
    ///
    /// If this is a Haskell project, this is the name of the package on Hackage. It is not a URL
    /// to the package, but rather the name of the package.
    HaskellPackage(String),

    /// Debian ITP (Intent To Package) bug number
    ///
    /// This is the bug number of the ITP bug in the Debian bug tracker. It is not a URL
    /// to the bug, but rather the bug number.
    DebianITP(i32),

    /// List of URLs to screenshots
    ///
    /// This is a list of URLs to screenshots of the project. It will be a list of
    /// URLs, which may be web pages or images.
    Screenshots(Vec<String>),

    /// Name of registry
    Registry(Vec<(String, String)>),

    /// Recommended way to cite the software
    ///
    /// This is the recommended way to cite the software, which may be a URL or a
    /// DOI.
    CiteAs(String),

    /// Link for donations (e.g. Paypal, Libera, etc)
    ///
    /// This is a URL to a donation page, which should be a web page.
    /// It is different from the Funding field, which describes
    /// the funding the project has received.
    Donation(String),

    /// Link to a life instance of the webservice
    ///
    /// This is the URL to the live instance of the project. This should generally
    /// be the canonical instance of the project.
    ///
    /// For demo instances, see the Demo field.
    Webservice(String),

    /// Name of the buildsystem used
    ///
    /// This is the name of the buildsystem used by the project. E.g. "make", "cmake",
    /// "meson", etc
    BuildSystem(String),

    /// FAQ
    ///
    /// This is the URL to the project's FAQ, which may be a web page or a file.
    FAQ(String),
}

#[derive(PartialEq, Eq, Debug, Clone)]
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
            UpstreamDatum::APIDocumentation(..) => "API-Documentation",
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
            UpstreamDatum::BuildSystem(..) => "BuildSystem",
            UpstreamDatum::FAQ(..) => "FAQ",
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
            UpstreamDatum::APIDocumentation(s) => Some(s),
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
            UpstreamDatum::BuildSystem(b) => Some(b),
            UpstreamDatum::FAQ(f) => Some(f),
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
            UpstreamDatum::APIDocumentation(s) => Some(s.parse().ok()?),
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
            UpstreamDatum::BuildSystem(_) => None,
            UpstreamDatum::FAQ(f) => Some(f.parse().ok()?),
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
            UpstreamDatum::APIDocumentation(s) => write!(f, "API-Documentation: {}", s),
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
            UpstreamDatum::BuildSystem(bs) => {
                write!(f, "BuildSystem: {}", bs)
            }
            UpstreamDatum::FAQ(faq) => {
                write!(f, "FAQ: {}", faq)
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
            UpstreamDatum::APIDocumentation(s) => serializer.serialize_str(s),
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
            UpstreamDatum::BuildSystem(bs) => serializer.serialize_str(bs),
            UpstreamDatum::FAQ(faq) => serializer.serialize_str(faq),
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct UpstreamMetadata(Vec<UpstreamDatumWithMetadata>);

impl UpstreamMetadata {
    pub fn new() -> Self {
        UpstreamMetadata(Vec::new())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn sort(&mut self) {
        self.0.sort_by(|a, b| a.datum.field().cmp(b.datum.field()));
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

    pub fn name(&self) -> Option<&str> {
        self.get("Name").and_then(|d| d.datum.as_str())
    }

    pub fn homepage(&self) -> Option<&str> {
        self.get("Homepage").and_then(|d| d.datum.as_str())
    }

    pub fn repository(&self) -> Option<&str> {
        self.get("Repository").and_then(|d| d.datum.as_str())
    }

    pub fn repository_browse(&self) -> Option<&str> {
        self.get("Repository-Browse").and_then(|d| d.datum.as_str())
    }

    pub fn description(&self) -> Option<&str> {
        self.get("Description").and_then(|d| d.datum.as_str())
    }

    pub fn summary(&self) -> Option<&str> {
        self.get("Summary").and_then(|d| d.datum.as_str())
    }

    pub fn license(&self) -> Option<&str> {
        self.get("License").and_then(|d| d.datum.as_str())
    }

    pub fn author(&self) -> Option<&Vec<Person>> {
        self.get("Author").map(|d| match &d.datum {
            UpstreamDatum::Author(authors) => authors,
            _ => unreachable!(),
        })
    }

    pub fn maintainer(&self) -> Option<&Person> {
        self.get("Maintainer").map(|d| match &d.datum {
            UpstreamDatum::Maintainer(maintainer) => maintainer,
            _ => unreachable!(),
        })
    }

    pub fn bug_database(&self) -> Option<&str> {
        self.get("Bug-Database").and_then(|d| d.datum.as_str())
    }

    pub fn bug_submit(&self) -> Option<&str> {
        self.get("Bug-Submit").and_then(|d| d.datum.as_str())
    }

    pub fn contact(&self) -> Option<&str> {
        self.get("Contact").and_then(|d| d.datum.as_str())
    }

    pub fn cargo_crate(&self) -> Option<&str> {
        self.get("Cargo-Crate").and_then(|d| d.datum.as_str())
    }

    pub fn security_md(&self) -> Option<&str> {
        self.get("Security-MD").and_then(|d| d.datum.as_str())
    }

    pub fn security_contact(&self) -> Option<&str> {
        self.get("Security-Contact").and_then(|d| d.datum.as_str())
    }

    pub fn version(&self) -> Option<&str> {
        self.get("Version").and_then(|d| d.datum.as_str())
    }

    pub fn keywords(&self) -> Option<&Vec<String>> {
        self.get("Keywords").map(|d| match &d.datum {
            UpstreamDatum::Keywords(keywords) => keywords,
            _ => unreachable!(),
        })
    }

    pub fn documentation(&self) -> Option<&str> {
        self.get("Documentation").and_then(|d| d.datum.as_str())
    }

    pub fn api_documentation(&self) -> Option<&str> {
        self.get("API-Documentation").and_then(|d| d.datum.as_str())
    }

    pub fn go_import_path(&self) -> Option<&str> {
        self.get("Go-Import-Path").and_then(|d| d.datum.as_str())
    }

    pub fn download(&self) -> Option<&str> {
        self.get("Download").and_then(|d| d.datum.as_str())
    }

    pub fn wiki(&self) -> Option<&str> {
        self.get("Wiki").and_then(|d| d.datum.as_str())
    }

    pub fn mailing_list(&self) -> Option<&str> {
        self.get("MailingList").and_then(|d| d.datum.as_str())
    }

    pub fn sourceforge_project(&self) -> Option<&str> {
        self.get("SourceForge-Project")
            .and_then(|d| d.datum.as_str())
    }

    pub fn archive(&self) -> Option<&str> {
        self.get("Archive").and_then(|d| d.datum.as_str())
    }

    pub fn demo(&self) -> Option<&str> {
        self.get("Demo").and_then(|d| d.datum.as_str())
    }

    pub fn pecl_package(&self) -> Option<&str> {
        self.get("Pecl-Package").and_then(|d| d.datum.as_str())
    }

    pub fn haskell_package(&self) -> Option<&str> {
        self.get("Haskell-Package").and_then(|d| d.datum.as_str())
    }

    pub fn funding(&self) -> Option<&str> {
        self.get("Funding").and_then(|d| d.datum.as_str())
    }

    pub fn changelog(&self) -> Option<&str> {
        self.get("Changelog").and_then(|d| d.datum.as_str())
    }

    pub fn debian_itp(&self) -> Option<i32> {
        self.get("Debian-ITP").and_then(|d| match &d.datum {
            UpstreamDatum::DebianITP(itp) => Some(*itp),
            _ => unreachable!(),
        })
    }

    pub fn screenshots(&self) -> Option<&Vec<String>> {
        self.get("Screenshots").map(|d| match &d.datum {
            UpstreamDatum::Screenshots(screenshots) => screenshots,
            _ => unreachable!(),
        })
    }

    pub fn donation(&self) -> Option<&str> {
        self.get("Donation").and_then(|d| d.datum.as_str())
    }

    pub fn cite_as(&self) -> Option<&str> {
        self.get("Cite-As").and_then(|d| d.datum.as_str())
    }

    pub fn registry(&self) -> Option<&Vec<(String, String)>> {
        self.get("Registry").map(|d| match &d.datum {
            UpstreamDatum::Registry(registry) => registry,
            _ => unreachable!(),
        })
    }

    pub fn webservice(&self) -> Option<&str> {
        self.get("Webservice").and_then(|d| d.datum.as_str())
    }

    pub fn buildsystem(&self) -> Option<&str> {
        self.get("BuildSystem").and_then(|d| d.datum.as_str())
    }

    pub fn copyright(&self) -> Option<&str> {
        self.get("Copyright").and_then(|d| d.datum.as_str())
    }

    pub fn faq(&self) -> Option<&str> {
        self.get("FAQ").and_then(|d| d.datum.as_str())
    }
}

impl std::ops::Index<&str> for UpstreamMetadata {
    type Output = UpstreamDatumWithMetadata;

    fn index(&self, index: &str) -> &Self::Output {
        self.get(index).unwrap()
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

impl From<UpstreamDatum> for UpstreamDatumWithMetadata {
    fn from(d: UpstreamDatum) -> Self {
        UpstreamDatumWithMetadata {
            datum: d,
            certainty: None,
            origin: None,
        }
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

#[cfg(feature = "pyo3")]
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
    Timeout(tokio::time::Duration),
    Error {
        url: reqwest::Url,
        status: u16,
        response: reqwest::Response,
    },
}

impl std::fmt::Display for HTTPJSONError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            HTTPJSONError::HTTPError(e) => write!(f, "{}", e),
            HTTPJSONError::Timeout(timeout) => write!(f, "Timeout after {:?}", timeout),
            HTTPJSONError::Error {
                url,
                status,
                response: _,
            } => write!(f, "HTTP error {} for {}:", status, url,),
        }
    }
}

pub async fn load_json_url(
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
        .default_headers(headers)
        .build()
        .map_err(HTTPJSONError::HTTPError)?;

    let http_url: reqwest::Url = Into::<String>::into(http_url.clone()).parse().unwrap();

    let request = client
        .get(http_url)
        .build()
        .map_err(HTTPJSONError::HTTPError)?;

    let timeout = timeout.unwrap_or(std::time::Duration::from_secs(30));

    let response = tokio::time::timeout(timeout, client.execute(request))
        .await
        .map_err(|_| HTTPJSONError::Timeout(timeout))?
        .map_err(HTTPJSONError::HTTPError)?;

    if !response.status().is_success() {
        return Err(HTTPJSONError::Error {
            url: response.url().clone(),
            status: response.status().as_u16(),
            response,
        });
    }

    let json_contents: serde_json::Value =
        response.json().await.map_err(HTTPJSONError::HTTPError)?;

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

pub async fn check_url_canonical(url: &Url) -> Result<Url, CanonicalizeError> {
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(CanonicalizeError::Unverifiable(
            url.clone(),
            format!("Unsupported scheme {}", url.scheme()),
        ));
    }

    let client = crate::http::build_client()
        .build()
        .map_err(|e| CanonicalizeError::Unverifiable(url.clone(), format!("HTTP error {}", e)))?;

    let response =
        client.get(url.clone()).send().await.map_err(|e| {
            CanonicalizeError::Unverifiable(url.clone(), format!("HTTP error {}", e))
        })?;

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

#[async_trait::async_trait]
pub trait Forge: Send + Sync {
    fn repository_browse_can_be_homepage(&self) -> bool;

    fn name(&self) -> &'static str;

    fn bug_database_url_from_bug_submit_url(&self, _url: &Url) -> Option<Url> {
        None
    }

    fn bug_submit_url_from_bug_database_url(&self, _url: &Url) -> Option<Url> {
        None
    }

    async fn check_bug_database_canonical(&self, url: &Url) -> Result<Url, CanonicalizeError> {
        Err(CanonicalizeError::Unverifiable(
            url.clone(),
            "Not implemented".to_string(),
        ))
    }

    async fn check_bug_submit_url_canonical(&self, url: &Url) -> Result<Url, CanonicalizeError> {
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

    async fn extend_metadata(
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

#[async_trait::async_trait]
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

    async fn check_bug_database_canonical(&self, url: &Url) -> Result<Url, CanonicalizeError> {
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

        let response = match reqwest::get(api_url).await {
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
        let data = response.json::<serde_json::Value>().await.map_err(|e| {
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

    async fn check_bug_submit_url_canonical(&self, url: &Url) -> Result<Url, CanonicalizeError> {
        let mut path_segments = url.path_segments().unwrap().collect::<Vec<_>>();
        path_segments.pop();
        let db_url = with_path_segments(url, &path_segments).unwrap();
        let mut canonical_db_url = self.check_bug_database_canonical(&db_url).await?;
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

#[async_trait::async_trait]
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

    async fn check_bug_database_canonical(&self, url: &Url) -> Result<Url, CanonicalizeError> {
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
        match load_json_url(&api_url, None).await {
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

                check_url_canonical(&canonical_url).await
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

    async fn check_bug_submit_url_canonical(&self, url: &Url) -> Result<Url, CanonicalizeError> {
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
        let mut canonical_db_url = self.check_bug_database_canonical(&db_url).await?;
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

async fn extend_from_external_guesser<
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Vec<UpstreamDatum>>,
>(
    metadata: &mut Vec<UpstreamDatumWithMetadata>,
    max_certainty: Option<Certainty>,
    supported_fields: &[&str],
    new_items: F,
) {
    if max_certainty.is_some()
        && !possible_fields_missing(metadata, supported_fields, max_certainty.unwrap())
    {
        return;
    }

    let new_items = new_items()
        .await
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

#[async_trait::async_trait]
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

    async fn extend_metadata(
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
            || async {
                crate::forges::sourceforge::guess_from_sf(project, subproject.as_deref()).await
            },
        )
        .await
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

pub async fn find_forge(url: &Url, net_access: Option<bool>) -> Option<Box<dyn Forge>> {
    if url.host_str()? == "sourceforge.net" {
        return Some(Box::new(SourceForge::new()));
    }

    if url.host_str()?.ends_with(".launchpad.net") {
        return Some(Box::new(Launchpad::new()));
    }

    if url.host_str()? == "github.com" {
        return Some(Box::new(GitHub::new()));
    }

    if vcs::is_gitlab_site(url.host_str()?, net_access).await {
        return Some(Box::new(GitLab::new()));
    }

    None
}

pub async fn check_bug_database_canonical(
    url: &Url,
    net_access: Option<bool>,
) -> Result<Url, CanonicalizeError> {
    if let Some(forge) = find_forge(url, net_access).await {
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

pub async fn bug_submit_url_from_bug_database_url(
    url: &Url,
    net_access: Option<bool>,
) -> Option<Url> {
    if let Some(forge) = find_forge(url, net_access).await {
        forge.bug_submit_url_from_bug_database_url(url)
    } else {
        None
    }
}

pub async fn bug_database_url_from_bug_submit_url(
    url: &Url,
    net_access: Option<bool>,
) -> Option<Url> {
    if let Some(forge) = find_forge(url, net_access).await {
        forge.bug_database_url_from_bug_submit_url(url)
    } else {
        None
    }
}

pub async fn guess_bug_database_url_from_repo_url(
    url: &Url,
    net_access: Option<bool>,
) -> Option<Url> {
    if let Some(forge) = find_forge(url, net_access).await {
        forge.bug_database_url_from_repo_url(url)
    } else {
        None
    }
}

pub async fn repo_url_from_merge_request_url(url: &Url, net_access: Option<bool>) -> Option<Url> {
    if let Some(forge) = find_forge(url, net_access).await {
        forge.repo_url_from_merge_request_url(url)
    } else {
        None
    }
}

pub async fn bug_database_from_issue_url(url: &Url, net_access: Option<bool>) -> Option<Url> {
    if let Some(forge) = find_forge(url, net_access).await {
        forge.bug_database_from_issue_url(url)
    } else {
        None
    }
}

pub async fn check_bug_submit_url_canonical(
    url: &Url,
    net_access: Option<bool>,
) -> Result<Url, CanonicalizeError> {
    if let Some(forge) = find_forge(url, net_access).await {
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
    if let Some(sf_project) = crate::forges::sourceforge::extract_sf_project_name(url) {
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

pub async fn get_repology_metadata(srcname: &str, repo: Option<&str>) -> Option<serde_json::Value> {
    let repo = repo.unwrap_or("debian_unstable");
    let url = format!(
        "https://repology.org/tools/project-by?repo={}&name_type=srcname'
           '&target_page=api_v1_project&name={}",
        repo, srcname
    );

    match load_json_url(&Url::parse(url.as_str()).unwrap(), None).await {
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

#[cfg(feature = "pyo3")]
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
            "API-Documentation" => Ok(UpstreamDatum::APIDocumentation(val.extract::<String>()?)),
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
            "BuildSystem" => Ok(UpstreamDatum::BuildSystem(val.extract::<String>()?)),
            "FAQ" => Ok(UpstreamDatum::FAQ(val.extract::<String>()?)),
            _ => Err(PyRuntimeError::new_err(format!("Unknown field: {}", field))),
        }
    }
}

#[cfg(feature = "pyo3")]
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
                UpstreamDatum::APIDocumentation(a) => a.into_py(py),
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
                UpstreamDatum::BuildSystem(b) => b.to_object(py),
                UpstreamDatum::FAQ(f) => f.to_object(py),
            },
        )
            .to_object(py)
    }
}

#[cfg(feature = "pyo3")]
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
    ExtrapolationLimitExceeded(usize),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ProviderError::ParseError(e) => write!(f, "Parse error: {}", e),
            ProviderError::IoError(e) => write!(f, "IO error: {}", e),
            ProviderError::Other(e) => write!(f, "Other error: {}", e),
            ProviderError::HttpJsonError(e) => write!(f, "HTTP JSON error: {}", e),
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
            ProviderError::ExtrapolationLimitExceeded(e) => {
                PyRuntimeError::new_err((e.to_string(),))
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
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

type OldAsyncGuesser = fn(
    PathBuf,
    GuesserSettings,
) -> Pin<
    Box<
        dyn std::future::Future<Output = Result<Vec<UpstreamDatumWithMetadata>, ProviderError>>
            + Send,
    >,
>;

const OLD_STATIC_GUESSERS: &[(&str, OldAsyncGuesser)] = &[
    #[cfg(feature = "debian")]
    ("debian/watch", |path, settings| {
        Box::pin(async move {
            crate::providers::debian::guess_from_debian_watch(&path, &settings).await
        })
    }),
    #[cfg(feature = "debian")]
    ("debian/control", |path, settings| {
        Box::pin(
            async move { crate::providers::debian::guess_from_debian_control(&path, &settings) },
        )
    }),
    #[cfg(feature = "debian")]
    ("debian/changelog", |path, settings| {
        Box::pin(async move {
            crate::providers::debian::guess_from_debian_changelog(&path, &settings).await
        })
    }),
    #[cfg(feature = "debian")]
    ("debian/rules", |path, settings| {
        Box::pin(async move { crate::providers::debian::guess_from_debian_rules(&path, &settings) })
    }),
    #[cfg(feature = "python-pkginfo")]
    ("PKG-INFO", |path, settings| {
        Box::pin(
            async move { crate::providers::python::guess_from_pkg_info(&path, &settings).await },
        )
    }),
    ("package.json", |path, settings| {
        Box::pin(async move {
            crate::providers::package_json::guess_from_package_json(&path, &settings)
        })
    }),
    ("composer.json", |path, settings| {
        Box::pin(async move {
            crate::providers::composer_json::guess_from_composer_json(&path, &settings)
        })
    }),
    ("package.xml", |path, settings| {
        Box::pin(
            async move { crate::providers::package_xml::guess_from_package_xml(&path, &settings) },
        )
    }),
    ("package.yaml", |path, settings| {
        Box::pin(async move {
            crate::providers::package_yaml::guess_from_package_yaml(&path, &settings)
        })
    }),
    #[cfg(feature = "dist-ini")]
    ("dist.ini", |path, settings| {
        Box::pin(async move { crate::providers::perl::guess_from_dist_ini(&path, &settings) })
    }),
    #[cfg(feature = "debian")]
    ("debian/copyright", |path, settings| {
        Box::pin(async move {
            crate::providers::debian::guess_from_debian_copyright(&path, &settings).await
        })
    }),
    ("META.json", |path, settings| {
        Box::pin(async move { crate::providers::perl::guess_from_meta_json(&path, &settings) })
    }),
    ("MYMETA.json", |path, settings| {
        Box::pin(async move { crate::providers::perl::guess_from_meta_json(&path, &settings) })
    }),
    ("META.yml", |path, settings| {
        Box::pin(async move { crate::providers::perl::guess_from_meta_yml(&path, &settings) })
    }),
    ("MYMETA.yml", |path, settings| {
        Box::pin(async move { crate::providers::perl::guess_from_meta_yml(&path, &settings) })
    }),
    ("configure", |path, settings| {
        Box::pin(async move { crate::providers::autoconf::guess_from_configure(&path, &settings) })
    }),
    #[cfg(feature = "r-description")]
    ("DESCRIPTION", |path, settings| {
        Box::pin(
            async move { crate::providers::r::guess_from_r_description(&path, &settings).await },
        )
    }),
    #[cfg(feature = "cargo")]
    ("Cargo.toml", |path, settings| {
        Box::pin(async move { crate::providers::rust::guess_from_cargo(&path, &settings) })
    }),
    ("pom.xml", |path, settings| {
        Box::pin(async move { crate::providers::maven::guess_from_pom_xml(&path, &settings) })
    }),
    #[cfg(feature = "git-config")]
    (".git/config", |path, settings| {
        Box::pin(async move { crate::providers::git::guess_from_git_config(&path, &settings) })
    }),
    ("debian/get-orig-source.sh", |path, settings| {
        Box::pin(async move { crate::vcs_command::guess_from_get_orig_source(&path, &settings) })
    }),
    #[cfg(feature = "pyproject-toml")]
    ("pyproject.toml", |path, settings| {
        Box::pin(
            async move { crate::providers::python::guess_from_pyproject_toml(&path, &settings) },
        )
    }),
    #[cfg(feature = "setup-cfg")]
    ("setup.cfg", |path, settings| {
        Box::pin(
            async move { crate::providers::python::guess_from_setup_cfg(&path, &settings).await },
        )
    }),
    ("go.mod", |path, settings| {
        Box::pin(async move { crate::providers::go::guess_from_go_mod(&path, &settings) })
    }),
    ("Makefile.PL", |path, settings| {
        Box::pin(async move { crate::providers::perl::guess_from_makefile_pl(&path, &settings) })
    }),
    ("wscript", |path, settings| {
        Box::pin(async move { crate::providers::waf::guess_from_wscript(&path, &settings) })
    }),
    ("AUTHORS", |path, settings| {
        Box::pin(async move { crate::providers::authors::guess_from_authors(&path, &settings) })
    }),
    ("INSTALL", |path, settings| {
        Box::pin(async move { crate::providers::guess_from_install(&path, &settings).await })
    }),
    ("pubspec.yaml", |path, settings| {
        Box::pin(
            async move { crate::providers::pubspec::guess_from_pubspec_yaml(&path, &settings) },
        )
    }),
    ("pubspec.yml", |path, settings| {
        Box::pin(
            async move { crate::providers::pubspec::guess_from_pubspec_yaml(&path, &settings) },
        )
    }),
    ("meson.build", |path, settings| {
        Box::pin(async move { crate::providers::meson::guess_from_meson(&path, &settings) })
    }),
    ("metadata.json", |path, settings| {
        Box::pin(async move {
            crate::providers::metadata_json::guess_from_metadata_json(&path, &settings)
        })
    }),
    (".travis.yml", |path, settings| {
        Box::pin(async move { crate::guess_from_travis_yml(&path, &settings) })
    }),
];

fn find_guessers(path: &std::path::Path) -> Vec<Box<dyn Guesser>> {
    let mut candidates: Vec<Box<dyn Guesser>> = Vec::new();

    let path = path.canonicalize().unwrap();

    for (name, cb) in OLD_STATIC_GUESSERS {
        let subpath = path.join(name);
        if subpath.exists() {
            candidates.push(Box::new(PathGuesser {
                name: name.to_string(),
                subpath: subpath.clone(),
                cb: Box::new(move |p, s| Box::pin(cb(p.to_path_buf(), s.clone()))),
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
                    let name = name.to_string();
                    Box::pin(async move {
                        crate::providers::security_md::guess_from_security_md(&name, &p, &s)
                    })
                }),
            }));
        }
    }

    let mut found_pkg_info = path.join("PKG-INFO").exists();
    #[cfg(feature = "python-pkginfo")]
    for entry in std::fs::read_dir(&path).unwrap() {
        let entry = entry.unwrap();
        let filename = entry.file_name().to_string_lossy().to_string();
        if filename.ends_with(".egg-info") {
            candidates.push(Box::new(PathGuesser {
                name: format!("{}/PKG-INFO", filename),
                subpath: entry.path().join("PKG-INFO"),
                cb: Box::new(|p, s| {
                    Box::pin(
                        async move { crate::providers::python::guess_from_pkg_info(&p, &s).await },
                    )
                }),
            }));
            found_pkg_info = true;
        } else if filename.ends_with(".dist-info") {
            candidates.push(Box::new(PathGuesser {
                name: format!("{}/METADATA", filename),
                subpath: entry.path().join("METADATA"),
                cb: Box::new(|p, s| {
                    Box::pin(
                        async move { crate::providers::python::guess_from_pkg_info(&p, &s).await },
                    )
                }),
            }));
            found_pkg_info = true;
        }
    }

    #[cfg(feature = "pyo3")]
    if !found_pkg_info && path.join("setup.py").exists() {
        candidates.push(Box::new(PathGuesser {
            name: "setup.py".to_string(),
            subpath: path.join("setup.py"),
            cb: Box::new(|path, s| {
                Box::pin(async move {
                    crate::providers::python::guess_from_setup_py(&path, s.trust_package).await
                })
            }),
        }));
    }

    for entry in std::fs::read_dir(&path).unwrap() {
        let entry = entry.unwrap();

        if entry.file_name().to_string_lossy().ends_with(".gemspec") {
            candidates.push(Box::new(PathGuesser {
                name: entry.file_name().to_string_lossy().to_string(),
                subpath: entry.path(),
                cb: Box::new(|p, s| {
                    Box::pin(
                        async move { crate::providers::ruby::guess_from_gemspec(&p, &s).await },
                    )
                }),
            }));
        }
    }

    // TODO(jelmer): Perhaps scan all directories if no other primary project information file has been found?
    #[cfg(feature = "r-description")]
    for entry in std::fs::read_dir(&path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if entry.file_type().unwrap().is_dir() {
            let description_name = format!("{}/DESCRIPTION", entry.file_name().to_string_lossy());
            if path.join(&description_name).exists() {
                candidates.push(Box::new(PathGuesser {
                    name: description_name,
                    subpath: path.join("DESCRIPTION"),
                    cb: Box::new(|p, s| {
                        Box::pin(async move {
                            crate::providers::r::guess_from_r_description(&p, &s).await
                        })
                    }),
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
            cb: Box::new(|p, s| {
                Box::pin(
                    async move { crate::providers::doap::guess_from_doap(&p, s.trust_package) },
                )
            }),
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
                Box::pin(async move {
                    crate::providers::metainfo::guess_from_metainfo(&p, s.trust_package)
                })
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
                Box::pin(async move {
                    crate::providers::haskell::guess_from_cabal(&path, s.trust_package)
                })
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
            cb: Box::new(|path, s| {
                Box::pin(
                    async move { crate::readme::guess_from_readme(&path, s.trust_package).await },
                )
            }),
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
                Box::pin(async move {
                    crate::providers::nuspec::guess_from_nuspec(&path, s.trust_package).await
                })
            }),
        }));
    } else if nuspec_filenames.len() > 1 {
        log::warn!(
            "Multiple nuspec files found: {:?}, ignoring all.",
            nuspec_filenames
        );
    }

    #[cfg(feature = "opam")]
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

    #[cfg(feature = "opam")]
    match opam_filenames.len().cmp(&1) {
        Ordering::Equal => {
            let opam_filename = opam_filenames.remove(0);
            candidates.push(Box::new(PathGuesser {
                name: opam_filename.to_string_lossy().to_string(),
                subpath: path.join(&opam_filename),
                cb: Box::new(|path, s| {
                    Box::pin(async move {
                        crate::providers::ocaml::guess_from_opam(&path, s.trust_package)
                    })
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
            cb: Box::new(|path, s| {
                Box::pin(async move {
                    crate::providers::debian::guess_from_debian_patch(&path, &s).await
                })
            }),
        }));
    }

    candidates.push(Box::new(EnvironmentGuesser::new()));
    candidates.push(Box::new(PathGuesser {
        name: ".".to_string(),
        subpath: path.clone(),
        cb: Box::new(|p, s| Box::pin(async move { crate::guess_from_path(&p, &s) })),
    }));

    candidates
}

pub(crate) fn stream(
    path: &Path,
    config: &GuesserSettings,
    mut guessers: Vec<Box<dyn Guesser>>,
) -> impl Stream<Item = Result<UpstreamDatumWithMetadata, ProviderError>> {
    // For each of the guessers, stream from the guessers in parallel (using Guesser::stream
    // rather than Guesser::guess) and then return the results.
    let abspath = std::env::current_dir().unwrap().join(path);

    // Create streams for each of the guessers. Call stream on each one of them, manipulate
    let streams = guessers.iter_mut().map(move |guesser| {
        let abspath = abspath.clone();
        let config = config.clone();
        let stream = guesser.stream(&config);
        let guesser_name = guesser.name().to_string();
        stream.map(move |res| {
            res.map({
                let abspath = abspath.clone();
                let guesser_name = guesser_name.clone();
                move |mut v| {
                    rewrite_upstream_datum(&guesser_name, &mut v, &abspath);
                    v
                }
            })
        })
    });

    // Combine the streams into a single stream.
    futures::stream::select_all(streams)
}

fn rewrite_upstream_datum(
    guesser_name: &str,
    datum: &mut UpstreamDatumWithMetadata,
    abspath: &std::path::Path,
) {
    log::trace!("{}: {:?}", guesser_name, datum);
    datum.origin = datum
        .origin
        .clone()
        .or(Some(Origin::Other(guesser_name.to_string())));
    if let Some(Origin::Path(p)) = datum.origin.as_ref() {
        if let Ok(suffix) = p.strip_prefix(abspath) {
            if suffix.to_str().unwrap().is_empty() {
                datum.origin = Some(Origin::Path(PathBuf::from_str(".").unwrap()));
            } else {
                datum.origin = Some(Origin::Path(PathBuf::from_str(".").unwrap().join(suffix)));
            }
        }
    }
}

pub fn upstream_metadata_stream(
    path: &std::path::Path,
    trust_package: Option<bool>,
) -> impl Stream<Item = Result<UpstreamDatumWithMetadata, ProviderError>> {
    let trust_package = trust_package.unwrap_or(false);

    let guessers = find_guessers(path);

    stream(path, &GuesserSettings { trust_package }, guessers)
}

pub async fn extend_upstream_metadata(
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

        if let Some(project) =
            crate::forges::sourceforge::extract_sf_project_name(value.datum.as_str().unwrap())
        {
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
        SourceForge::new()
            .extend_metadata(
                upstream_metadata.mut_items(),
                sf_project.as_str(),
                sf_certainty,
            )
            .await;
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
            .await
            .unwrap();
    }

    let archive = upstream_metadata.get("Archive");
    #[cfg(feature = "cargo")]
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
            .await
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
            .await
            .unwrap();
    }

    #[cfg(feature = "debian")]
    if net_access && consult_external_directory {
        // TODO(jelmer): Don't assume debian/control exists
        let package = match debian_control::Control::from_file_relaxed(path.join("debian/control"))
        {
            Ok((control, _)) => control.source().and_then(|s| s.name()),
            Err(_) => None,
        };

        if let Some(package) = package {
            #[cfg(feature = "launchpad")]
            extend_from_lp(
                upstream_metadata.mut_items(),
                minimum_certainty,
                package.as_str(),
                None,
                None,
            )
            .await;
            crate::providers::arch::Aur::new()
                .extend_metadata(
                    upstream_metadata.mut_items(),
                    package.as_str(),
                    Some(minimum_certainty),
                )
                .await
                .unwrap();
            crate::providers::gobo::Gobo::new()
                .extend_metadata(
                    upstream_metadata.mut_items(),
                    package.as_str(),
                    Some(minimum_certainty),
                )
                .await
                .unwrap();
            extend_from_repology(
                upstream_metadata.mut_items(),
                minimum_certainty,
                package.as_str(),
            )
            .await;
        }
    }
    crate::extrapolate::extrapolate_fields(upstream_metadata, net_access, None).await?;
    Ok(())
}

#[async_trait::async_trait]
pub trait ThirdPartyRepository {
    fn name(&self) -> &'static str;
    fn supported_fields(&self) -> &'static [&'static str];
    fn max_supported_certainty(&self) -> Certainty;

    async fn extend_metadata(
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
            || async { self.guess_metadata(name).await.unwrap() },
        )
        .await;

        Ok(())
    }

    async fn guess_metadata(&self, name: &str) -> Result<Vec<UpstreamDatum>, ProviderError>;
}

#[cfg(feature = "launchpad")]
async fn extend_from_lp(
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

    extend_from_external_guesser(upstream_metadata, Some(lp_certainty), lp_fields, || async {
        crate::providers::launchpad::guess_from_launchpad(package, distribution, suite)
            .await
            .unwrap()
    })
    .await
}

async fn extend_from_repology(
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

    extend_from_external_guesser(
        upstream_metadata,
        Some(certainty),
        repology_fields,
        || async {
            crate::providers::repology::guess_from_repology(source_package)
                .await
                .unwrap()
        },
    )
    .await
}

/// Fix existing upstream metadata.
pub async fn fix_upstream_metadata(upstream_metadata: &mut UpstreamMetadata) {
    if let Some(repository) = upstream_metadata.get_mut("Repository") {
        let url = crate::vcs::sanitize_url(repository.datum.as_str().unwrap()).await;
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
pub async fn summarize_upstream_metadata(
    metadata_items: impl Stream<Item = UpstreamDatumWithMetadata>,
    path: &std::path::Path,
    net_access: Option<bool>,
    consult_external_directory: Option<bool>,
    check: Option<bool>,
) -> Result<UpstreamMetadata, ProviderError> {
    let check = check.unwrap_or(false);
    let mut upstream_metadata = UpstreamMetadata::new();

    let metadata_items = metadata_items.filter_map(|item| async move {
        let bad: bool = item.datum.known_bad_guess();
        if bad {
            log::debug!("Excluding known bad item {:?}", item);
            None
        } else {
            Some(item)
        }
    });

    let metadata_items = metadata_items.collect::<Vec<_>>().await;

    upstream_metadata.update(metadata_items.into_iter());

    extend_upstream_metadata(
        &mut upstream_metadata,
        path,
        None,
        net_access,
        consult_external_directory,
    )
    .await?;

    if check {
        check_upstream_metadata(&mut upstream_metadata, None).await;
    }

    fix_upstream_metadata(&mut upstream_metadata).await;

    // Sort by name
    upstream_metadata.sort();

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
) -> impl Stream<Item = Result<UpstreamDatumWithMetadata, ProviderError>> {
    let items = upstream_metadata_stream(path, trust_package);

    items.filter_map(move |e| async move {
        match e {
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
        }
    })
}

pub async fn get_upstream_info(
    path: &std::path::Path,
    trust_package: Option<bool>,
    net_access: Option<bool>,
    consult_external_directory: Option<bool>,
    check: Option<bool>,
) -> Result<UpstreamMetadata, ProviderError> {
    let metadata_items = upstream_metadata_stream(path, trust_package);

    let metadata_items = metadata_items.filter_map(|x| async {
        match x {
            Ok(x) => Some(x),
            Err(e) => {
                log::error!("{}", e);
                None
            }
        }
    });

    summarize_upstream_metadata(
        metadata_items,
        path,
        net_access,
        consult_external_directory,
        check,
    )
    .await
}

/// Guess the upstream metadata dictionary.
///
/// # Arguments
/// * `path`: Path to the package
/// * `trust_package`: Whether to trust the package contents and i.e. run executables in it
/// * `net_access`: Whether to allow net access
/// * `consult_external_directory`: Whether to pull in data from external (user-maintained) directories.
pub async fn guess_upstream_metadata(
    path: &std::path::Path,
    trust_package: Option<bool>,
    net_access: Option<bool>,
    consult_external_directory: Option<bool>,
    check: Option<bool>,
) -> Result<UpstreamMetadata, ProviderError> {
    let metadata_items = guess_upstream_metadata_items(path, trust_package, None);

    let metadata_items = metadata_items.filter_map(|x| async {
        match x {
            Ok(x) => Some(x),
            Err(e) => {
                log::error!("{}", e);
                None
            }
        }
    });
    summarize_upstream_metadata(
        metadata_items,
        path,
        net_access,
        consult_external_directory,
        check,
    )
    .await
}

pub async fn verify_screenshots(urls: &[&str]) -> Vec<(String, Option<bool>)> {
    let mut ret = Vec::new();
    for url in urls {
        let mut request = reqwest::Request::new(reqwest::Method::GET, url.parse().unwrap());
        request.headers_mut().insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static(USER_AGENT),
        );

        match reqwest::Client::new().execute(request).await {
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
pub async fn check_upstream_metadata(
    upstream_metadata: &mut UpstreamMetadata,
    version: Option<&str>,
) {
    let repository = upstream_metadata.get_mut("Repository");
    if let Some(repository) = repository {
        match vcs::check_repository_url_canonical(repository.datum.to_url().unwrap(), version).await
        {
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
                )
                .await;
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
        match check_url_canonical(&homepage.datum.to_url().unwrap()).await {
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
        match check_url_canonical(&repository_browse.datum.to_url().unwrap()).await {
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
        match check_bug_database_canonical(&bug_database.datum.to_url().unwrap(), Some(true)).await
        {
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
        match check_bug_submit_url_canonical(&bug_submit.datum.to_url().unwrap(), Some(true)).await
        {
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
        )
        .await
        {
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

#[async_trait::async_trait]
pub(crate) trait Guesser {
    fn name(&self) -> &str;

    /// Guess metadata from a given path.
    async fn guess(
        &mut self,
        settings: &GuesserSettings,
    ) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError>;

    fn stream(
        &mut self,
        settings: &GuesserSettings,
    ) -> Pin<Box<dyn Stream<Item = Result<UpstreamDatumWithMetadata, ProviderError>> + Send>> {
        let metadata = match futures::executor::block_on(self.guess(settings)) {
            Ok(metadata) => metadata,
            Err(e) => return futures::stream::once(async { Err(e) }).boxed(),
        };

        Box::pin(futures::stream::iter(metadata.into_iter().map(Ok)))
    }
}

pub struct PathGuesser {
    name: String,
    subpath: std::path::PathBuf,
    cb: Box<
        dyn FnMut(
                PathBuf,
                GuesserSettings,
            ) -> Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<Vec<UpstreamDatumWithMetadata>, ProviderError>,
                        > + Send,
                >,
            > + Send,
    >,
}

#[async_trait::async_trait]
impl Guesser for PathGuesser {
    fn name(&self) -> &str {
        &self.name
    }

    async fn guess(
        &mut self,
        settings: &GuesserSettings,
    ) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
        (self.cb)(self.subpath.clone(), settings.clone()).await
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

#[async_trait::async_trait]
impl Guesser for EnvironmentGuesser {
    fn name(&self) -> &str {
        "environment"
    }

    async fn guess(
        &mut self,
        _settings: &GuesserSettings,
    ) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
        crate::guess_from_environment()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upstream_metadata() {
        let mut data = UpstreamMetadata::new();
        assert_eq!(data.len(), 0);

        data.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Homepage("https://example.com".to_string()),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        assert_eq!(data.len(), 1);
        assert_eq!(
            data.get("Homepage").unwrap().datum.as_str().unwrap(),
            "https://example.com"
        );

        assert_eq!(data.homepage(), Some("https://example.com"));
    }

    #[tokio::test]
    async fn test_bug_database_url_from_bug_submit_url() {
        let url = Url::parse("https://bugs.launchpad.net/bugs/+filebug").unwrap();
        assert_eq!(
            bug_database_url_from_bug_submit_url(&url, None)
                .await
                .unwrap(),
            Url::parse("https://bugs.launchpad.net/bugs").unwrap()
        );

        let url = Url::parse("https://github.com/dulwich/dulwich/issues/new").unwrap();

        assert_eq!(
            bug_database_url_from_bug_submit_url(&url, None)
                .await
                .unwrap(),
            Url::parse("https://github.com/dulwich/dulwich/issues").unwrap()
        );

        let url = Url::parse("https://sourceforge.net/p/dulwich/bugs/new").unwrap();

        assert_eq!(
            bug_database_url_from_bug_submit_url(&url, None)
                .await
                .unwrap(),
            Url::parse("https://sourceforge.net/p/dulwich/bugs").unwrap()
        );
    }

    #[test]
    fn test_person_from_str() {
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
