use log::warn;
use pyo3::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    BugDatabase(String),
    BugSubmit(String),
    Contact(String),
    CargoCrate(String),
    SecurityMD(String),
    SecurityContact(String),
    Version(String),
}

pub struct UpstreamDatumWithMetadata {
    pub datum: UpstreamDatum,
    pub origin: Option<String>,
    pub certainty: Option<Certainty>,
}

impl UpstreamDatum {
    pub fn field(&self) -> &'static str {
        match self {
            UpstreamDatum::Summary(..) => "X-Summary",
            UpstreamDatum::Description(..) => "X-Description",
            UpstreamDatum::Name(..) => "Name",
            UpstreamDatum::Homepage(..) => "Homepage",
            UpstreamDatum::Repository(..) => "Repository",
            UpstreamDatum::RepositoryBrowse(..) => "Repository-Browse",
            UpstreamDatum::License(..) => "License",
            UpstreamDatum::Author(..) => "Author",
            UpstreamDatum::BugDatabase(..) => "Bug-Database",
            UpstreamDatum::BugSubmit(..) => "Bug-Submit",
            UpstreamDatum::Contact(..) => "Contact",
            UpstreamDatum::CargoCrate(..) => "X-Cargo-Crate",
            UpstreamDatum::SecurityMD(..) => "X-Security-MD",
            UpstreamDatum::SecurityContact(..) => "X-Security-Contact",
            UpstreamDatum::Version(..) => "Version",
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
