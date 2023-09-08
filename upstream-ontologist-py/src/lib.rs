use log::debug;
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::exceptions::PyRuntimeError;
use pyo3::import_exception;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::HashMap;
use std::path::PathBuf;
use upstream_ontologist::{
    py_to_upstream_datum, CanonicalizeError, Person, UpstreamDatum, UpstreamDatumWithMetadata,
};
use url::Url;

import_exception!(urllib.error, HTTPError);
create_exception!(upstream_ontologist, UnverifiableUrl, PyException);
create_exception!(upstream_ontologist, InvalidUrl, PyException);
create_exception!(upstream_ontologist, NoSuchRepologyProject, PyException);
create_exception!(upstream_ontologist, NoSuchForgeProject, PyException);
create_exception!(upstream_ontologist, ParseError, PyException);

#[pyfunction]
fn url_from_git_clone_command(command: &[u8]) -> Option<String> {
    upstream_ontologist::url_from_git_clone_command(command)
}

#[pyfunction]
fn url_from_fossil_clone_command(command: &[u8]) -> Option<String> {
    upstream_ontologist::url_from_fossil_clone_command(command)
}

#[pyfunction]
fn url_from_svn_co_command(command: &[u8]) -> Option<String> {
    upstream_ontologist::url_from_svn_co_command(command)
}

#[pyfunction]
fn drop_vcs_in_scheme(url: &str) -> &str {
    upstream_ontologist::vcs::drop_vcs_in_scheme(url)
}

fn upstream_datum_to_py(py: Python, datum: UpstreamDatum) -> PyResult<(String, PyObject)> {
    let m = PyModule::import(py, "upstream_ontologist.guess")?;
    let PersonCls = m.getattr("Person")?;
    Ok((
        datum.field().to_string(),
        match datum {
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
            UpstreamDatum::Keywords(ks) => ks.into_py(py),
            UpstreamDatum::Copyright(c) => c.into_py(py),
            UpstreamDatum::Documentation(a) => a.into_py(py),
            UpstreamDatum::GoImportPath(ip) => ip.into_py(py),
            UpstreamDatum::Archive(a) => a.into_py(py),
            UpstreamDatum::Demo(d) => d.into_py(py),
            UpstreamDatum::Maintainer(m) => PersonCls.call1((m.name, m.email, m.url))?.into_py(py),
            UpstreamDatum::Author(a) => a
                .into_iter()
                .map(|x| PersonCls.call1((x.name, x.email, x.url)))
                .collect::<PyResult<Vec<&PyAny>>>()?
                .into_py(py),
            UpstreamDatum::Wiki(w) => w.into_py(py),
            UpstreamDatum::Download(d) => d.into_py(py),
            UpstreamDatum::MailingList(m) => m.into_py(py),
            UpstreamDatum::SourceForgeProject(m) => m.into_py(py),
            UpstreamDatum::PeclPackage(p) => p.into_py(py),
            UpstreamDatum::Funding(p) => p.into_py(py),
            UpstreamDatum::Changelog(c) => c.into_py(py),
        },
    ))
}

fn upstream_datum_with_metadata_to_py(
    py: Python,
    datum: UpstreamDatumWithMetadata,
) -> PyResult<PyObject> {
    let m = PyModule::import(py, "upstream_ontologist.guess")?;

    let UpstreamDatumCls = m.getattr("UpstreamDatum")?;

    {
        let (field, py_datum) = upstream_datum_to_py(py, datum.datum)?;
        let datum = UpstreamDatumCls.call1((
            field,
            py_datum,
            datum.certainty.map(|x| x.to_string()),
            datum.origin,
        ))?;
        Ok(datum.to_object(py))
    }
}

#[pyfunction]
fn guess_from_meson(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret =
        upstream_ontologist::providers::meson::guess_from_meson(path.as_path(), trust_package)
            .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_package_json(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::providers::package_json::guess_from_package_json(
        path.as_path(),
        trust_package,
    )
    .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn debian_is_native(path: PathBuf) -> PyResult<Option<bool>> {
    Ok(upstream_ontologist::debian_is_native(path.as_path())?)
}

#[pyfunction]
fn unsplit_vcs_url(repo_url: &str, branch: Option<&str>, subpath: Option<&str>) -> String {
    upstream_ontologist::vcs::unsplit_vcs_url(repo_url, branch, subpath)
}

fn json_to_py(py: Python, data: serde_json::Value) -> PyResult<PyObject> {
    match data {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(b) => Ok(b.into_py(py)),
        serde_json::Value::Number(i) => Ok(i.as_i64().unwrap().into_py(py)),
        serde_json::Value::String(s) => Ok(s.into_py(py)),
        serde_json::Value::Array(a) => Ok(a
            .into_iter()
            .map(|x| json_to_py(py, x))
            .collect::<PyResult<Vec<PyObject>>>()?
            .into_py(py)),
        serde_json::Value::Object(o) => Ok({
            let d = PyDict::new(py);

            for (k, v) in o {
                d.set_item(k.into_py(py), json_to_py(py, v)?)?;
            }
            d.into_py(py)
        }),
    }
}

#[pyfunction]
fn load_json_url(py: Python, http_url: &str, timeout: Option<u64>) -> PyResult<PyObject> {
    debug!("Loading JSON from {}", http_url);
    let http_url = http_url
        .parse::<reqwest::Url>()
        .map_err(|e| PyRuntimeError::new_err(format!("{}: {}", e, http_url)))?;
    Ok(json_to_py(
        py,
        upstream_ontologist::load_json_url(&http_url, timeout.map(std::time::Duration::from_secs))
            .map_err(|e| match e {
                upstream_ontologist::HTTPJSONError::Error {
                    url,
                    status,
                    response,
                } => {
                    HTTPError::new_err((url.as_str().to_string(), status, response.text().unwrap()))
                }
                upstream_ontologist::HTTPJSONError::HTTPError(e) => {
                    PyRuntimeError::new_err(e.to_string())
                }
            })?,
    )?)
}

#[pyfunction]
fn guess_from_composer_json(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::providers::composer_json::guess_from_composer_json(
        path.as_path(),
        trust_package,
    )
    .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_package_xml(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::providers::package_xml::guess_from_package_xml(
        path.as_path(),
        trust_package,
    )
    .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_dist_ini(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret =
        upstream_ontologist::providers::perl::guess_from_dist_ini(path.as_path(), trust_package)
            .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_perl_dist_name(
    py: Python,
    path: PathBuf,
    dist_name: &str,
) -> PyResult<Vec<PyObject>> {
    let ret =
        upstream_ontologist::providers::perl::guess_from_perl_dist_name(path.as_path(), dist_name)
            .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_perl_module(py: Python, path: PathBuf) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::providers::perl::guess_from_perl_module(path.as_path())
        .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_pod(py: Python, contents: &str) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::providers::perl::guess_from_pod(contents)
        .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_pubspec_yaml(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::providers::pubspec::guess_from_pubspec_yaml(
        path.as_path(),
        trust_package,
    )
    .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_authors(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret =
        upstream_ontologist::providers::authors::guess_from_authors(path.as_path(), trust_package)
            .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_metadata_json(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_metadata_json(path.as_path(), trust_package)
        .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_meta_json(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret =
        upstream_ontologist::providers::perl::guess_from_meta_json(path.as_path(), trust_package)
            .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyclass(subclass)]
struct Forge(Box<dyn upstream_ontologist::Forge>);

#[pymethods]
impl Forge {
    #[getter]
    fn name(&self) -> PyResult<String> {
        Ok(self.0.name().to_string())
    }

    fn bug_database_url_from_bug_submit_url(&self, url: &str) -> PyResult<Option<String>> {
        let url = url.parse().unwrap();
        Ok(self
            .0
            .bug_database_url_from_bug_submit_url(&url)
            .map(|x| x.to_string()))
    }

    fn bug_submit_url_from_bug_database_url(&self, url: &str) -> PyResult<Option<String>> {
        let url = url.parse().unwrap();
        Ok(self
            .0
            .bug_submit_url_from_bug_database_url(&url)
            .map(|x| x.to_string()))
    }

    fn check_bug_database_canonical(&self, url: &str) -> PyResult<String> {
        let url = url.parse().unwrap();
        Ok(self
            .0
            .check_bug_database_canonical(&url)
            .map_err(|e| match e {
                CanonicalizeError::InvalidUrl(url, msg) => {
                    InvalidUrl::new_err((url.to_string(), msg))
                }
                CanonicalizeError::Unverifiable(url, msg) => {
                    UnverifiableUrl::new_err((url.to_string(), msg))
                }
                CanonicalizeError::RateLimited(url) => {
                    UnverifiableUrl::new_err((url.to_string(), "rate limited"))
                }
            })?
            .to_string())
    }

    fn check_bug_submit_url_canonical(&self, url: &str) -> PyResult<String> {
        let url = url.parse().unwrap();
        Ok(self
            .0
            .check_bug_submit_url_canonical(&url)
            .map_err(|e| match e {
                CanonicalizeError::InvalidUrl(url, msg) => {
                    InvalidUrl::new_err((url.to_string(), msg))
                }
                CanonicalizeError::Unverifiable(url, msg) => {
                    UnverifiableUrl::new_err((url.to_string(), msg))
                }
                CanonicalizeError::RateLimited(url) => {
                    UnverifiableUrl::new_err((url.to_string(), "rate limited"))
                }
            })?
            .to_string())
    }

    fn bug_database_from_issue_url(&self, url: &str) -> PyResult<Option<String>> {
        let url = url.parse().unwrap();
        Ok(self
            .0
            .bug_database_from_issue_url(&url)
            .map(|x| x.to_string()))
    }

    fn bug_database_url_from_repo_url(&self, url: &str) -> PyResult<Option<String>> {
        let url = url.parse().unwrap();
        Ok(self
            .0
            .bug_database_url_from_repo_url(&url)
            .map(|x| x.to_string()))
    }

    fn repo_url_from_merge_request_url(&self, url: &str) -> PyResult<Option<String>> {
        let url = url.parse().unwrap();
        Ok(self
            .0
            .repo_url_from_merge_request_url(&url)
            .map(|x| x.to_string()))
    }

    #[getter]
    fn repository_browse_can_be_homepage(&self) -> bool {
        self.0.repository_browse_can_be_homepage()
    }
}

#[pyclass(subclass,extends=Forge)]
struct GitHub;

#[pymethods]
impl GitHub {
    #[new]
    fn new() -> (Self, Forge) {
        let forge = upstream_ontologist::GitHub::new();
        (Self, Forge(Box::new(forge)))
    }
}

#[pyclass(subclass,extends=Forge)]
struct GitLab;

#[pymethods]
impl GitLab {
    #[new]
    fn new() -> (Self, Forge) {
        let forge = upstream_ontologist::GitLab::new();
        (Self, Forge(Box::new(forge)))
    }
}

#[pyclass(subclass,extends=Forge)]
struct Launchpad;

#[pymethods]
impl Launchpad {
    #[new]
    fn new() -> (Self, Forge) {
        let forge = upstream_ontologist::Launchpad::new();
        (Self, Forge(Box::new(forge)))
    }
}

#[pyclass(subclass,extends=Forge)]
struct SourceForge;

#[pymethods]
impl SourceForge {
    #[new]
    fn new() -> (Self, Forge) {
        let forge = upstream_ontologist::SourceForge::new();
        (Self, Forge(Box::new(forge)))
    }
}

#[pyfunction]
fn guess_from_travis_yml(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_travis_yml(path.as_path(), trust_package)
        .map_err(map_provider_err_to_py_err)?;
    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_meta_yml(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret =
        upstream_ontologist::providers::perl::guess_from_meta_yml(path.as_path(), trust_package)
            .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn metadata_from_itp_bug_body(py: Python, body: &str) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::providers::debian::metadata_from_itp_bug_body(body)
        .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_metainfo(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::providers::metainfo::guess_from_metainfo(
        path.as_path(),
        trust_package,
    )
    .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_doap(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::providers::doap::guess_from_doap(path.as_path(), trust_package)
        .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_opam(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::providers::ocaml::guess_from_opam(path.as_path(), trust_package)
        .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_pom_xml(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret =
        upstream_ontologist::providers::maven::guess_from_pom_xml(path.as_path(), trust_package)
            .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn plausible_vcs_url(url: &str) -> PyResult<bool> {
    Ok(upstream_ontologist::vcs::plausible_url(url))
}

#[pyfunction]
fn plausible_vcs_browse_url(url: &str) -> PyResult<bool> {
    Ok(upstream_ontologist::vcs::plausible_browse_url(url))
}

#[pyfunction]
fn guess_from_wscript(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret =
        upstream_ontologist::providers::waf::guess_from_wscript(path.as_path(), trust_package)
            .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_makefile_pl(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    let ret =
        upstream_ontologist::providers::perl::guess_from_makefile_pl(path.as_path(), trust_package)
            .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_go_mod(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::providers::go::guess_from_go_mod(path.as_path(), trust_package)
        .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_cabal(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret =
        upstream_ontologist::providers::haskell::guess_from_cabal(path.as_path(), trust_package)
            .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_cabal_lines(py: Python, lines: Vec<String>) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::providers::haskell::guess_from_cabal_lines(lines.into_iter())
        .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_git_config(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    let ret =
        upstream_ontologist::providers::git::guess_from_git_config(path.as_path(), trust_package)
            .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_aur(py: Python, package: &str) -> PyResult<Vec<(String, PyObject)>> {
    let ret = upstream_ontologist::providers::arch::guess_from_aur(package);

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
        .collect::<PyResult<Vec<_>>>()
}

#[pyfunction]
fn check_url_canonical(url: &str) -> PyResult<String> {
    Ok(upstream_ontologist::check_url_canonical(
        &Url::parse(url).map_err(|e| InvalidUrl::new_err((url.to_string(), e.to_string())))?,
    )
    .map_err(|e| match e {
        CanonicalizeError::InvalidUrl(u, m) => InvalidUrl::new_err((u.to_string(), m)),
        CanonicalizeError::Unverifiable(u, m) => UnverifiableUrl::new_err((u.to_string(), m)),
        CanonicalizeError::RateLimited(u) => {
            UnverifiableUrl::new_err((u.to_string(), "Rate limited"))
        }
    })?
    .to_string())
}

#[pyfunction]
fn guess_from_environment(py: Python) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_environment().map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

fn map_provider_err_to_py_err(e: upstream_ontologist::ProviderError) -> PyErr {
    match e {
        upstream_ontologist::ProviderError::IoError(e) => e.into(),
        upstream_ontologist::ProviderError::ParseError(e) => ParseError::new_err((e,)),
        upstream_ontologist::ProviderError::Other(e) => PyRuntimeError::new_err((e,)),
    }
}

#[pyfunction]
fn guess_from_nuspec(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_nuspec(path.as_path(), trust_package)
        .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_repo_from_url(url: &str, net_access: Option<bool>) -> PyResult<Option<String>> {
    let url = Url::parse(url);
    if let Err(e) = url {
        return Ok(None);
    }

    Ok(upstream_ontologist::vcs::guess_repo_from_url(
        &url.unwrap(),
        net_access,
    ))
}

#[pyfunction]
fn probe_gitlab_host(hostname: &str) -> bool {
    upstream_ontologist::vcs::probe_gitlab_host(hostname)
}

#[pyfunction]
fn is_gitlab_site(hostname: &str, net_access: Option<bool>) -> bool {
    upstream_ontologist::vcs::is_gitlab_site(hostname, net_access)
}

#[pyfunction]
fn check_repository_url_canonical(url: &str, version: Option<&str>) -> PyResult<String> {
    Ok(upstream_ontologist::vcs::check_repository_url_canonical(
        Url::parse(url).map_err(|e| PyRuntimeError::new_err(format!("Invalid URL: {}", e)))?,
        version,
    )
    .map_err(|e| match e {
        CanonicalizeError::InvalidUrl(u, m) => InvalidUrl::new_err((u.to_string(), m)),
        CanonicalizeError::Unverifiable(u, m) => UnverifiableUrl::new_err((u.to_string(), m)),
        CanonicalizeError::RateLimited(u) => {
            UnverifiableUrl::new_err((u.to_string(), "Rate limited"))
        }
    })?
    .to_string())
}

#[pyfunction]
fn probe_upstream_branch_url(url: &str, version: Option<&str>) -> Option<bool> {
    upstream_ontologist::vcs::probe_upstream_branch_url(
        &Url::parse(url).expect("URL parsing failed"),
        version,
    )
}

#[pyfunction]
fn guess_from_gemspec(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret =
        upstream_ontologist::providers::ruby::guess_from_gemspec(path.as_path(), trust_package)
            .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_launchpad(
    py: Python,
    package: &str,
    distribution: Option<&str>,
    suite: Option<&str>,
) -> PyResult<Vec<(String, PyObject)>> {
    let ret = upstream_ontologist::providers::launchpad::guess_from_launchpad(
        package,
        distribution,
        suite,
    );

    if ret.is_none() {
        return Ok(vec![]);
    }

    ret.unwrap()
        .into_iter()
        .map(|x| upstream_datum_to_py(py, x))
        .collect::<PyResult<Vec<_>>>()
}

#[pyfunction]
fn browse_url_from_repo_url(
    url: &str,
    branch: Option<&str>,
    subpath: Option<&str>,
    net_access: Option<bool>,
) -> Option<String> {
    upstream_ontologist::vcs::browse_url_from_repo_url(url, branch, subpath, net_access)
}

#[pyfunction]
fn canonical_git_repo_url(url: &str, net_access: Option<bool>) -> PyResult<String> {
    let url =
        Url::parse(url).map_err(|e| PyRuntimeError::new_err(format!("Invalid URL: {}", e)))?;
    Ok(upstream_ontologist::vcs::canonical_git_repo_url(&url, net_access).to_string())
}

#[pyfunction]
fn find_public_repo_url(url: &str, net_access: Option<bool>) -> PyResult<Option<String>> {
    Ok(upstream_ontologist::vcs::find_public_repo_url(
        url, net_access,
    ))
}

#[pyfunction]
fn guess_from_configure(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::providers::autoconf::guess_from_configure(
        path.as_path(),
        trust_package,
    )
    .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_r_description(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    let ret =
        upstream_ontologist::providers::r::guess_from_r_description(path.as_path(), trust_package)
            .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_cargo(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::providers::rust::guess_from_cargo(path.as_path(), trust_package)
        .map_err(map_provider_err_to_py_err)?;

    ret.into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn find_forge(url: &str, net_access: Option<bool>) -> Option<Forge> {
    let url = Url::parse(url).ok()?;

    let forge = upstream_ontologist::find_forge(&url, net_access);

    if let Some(forge) = forge {
        Some(Forge(forge))
    } else {
        None
    }
}

#[pyfunction]
fn repo_url_from_merge_request_url(url: &str, net_access: Option<bool>) -> Option<String> {
    let url = Url::parse(url).ok()?;
    upstream_ontologist::repo_url_from_merge_request_url(&url, net_access).map(|x| x.to_string())
}

#[pyfunction]
fn bug_database_from_issue_url(url: &str, net_access: Option<bool>) -> Option<String> {
    let url = Url::parse(url).ok()?;
    upstream_ontologist::bug_database_from_issue_url(&url, net_access).map(|x| x.to_string())
}

#[pyfunction]
fn guess_bug_database_url_from_repo_url(url: &str, net_access: Option<bool>) -> Option<String> {
    let url = Url::parse(url).ok()?;
    upstream_ontologist::guess_bug_database_url_from_repo_url(&url, net_access)
        .map(|x| x.to_string())
}

#[pyfunction]
fn bug_database_url_from_bug_submit_url(url: &str, net_access: Option<bool>) -> Option<String> {
    let url = Url::parse(url).ok()?;
    upstream_ontologist::bug_database_url_from_bug_submit_url(&url, net_access)
        .map(|x| x.to_string())
}

#[pyfunction]
fn bug_submit_url_from_bug_database_url(url: &str, net_access: Option<bool>) -> Option<String> {
    let url = Url::parse(url).ok()?;
    upstream_ontologist::bug_submit_url_from_bug_database_url(&url, net_access)
        .map(|x| x.to_string())
}

#[pyfunction]
fn check_bug_database_canonical(url: &str, net_access: Option<bool>) -> PyResult<String> {
    let url =
        Url::parse(url).map_err(|e| PyRuntimeError::new_err(format!("Invalid URL: {}", e)))?;
    upstream_ontologist::check_bug_database_canonical(&url, net_access)
        .map_err(|e| match e {
            CanonicalizeError::InvalidUrl(url, msg) => InvalidUrl::new_err((url.to_string(), msg)),
            CanonicalizeError::Unverifiable(url, msg) => {
                UnverifiableUrl::new_err((url.to_string(), msg))
            }
            CanonicalizeError::RateLimited(url) => {
                UnverifiableUrl::new_err((url.to_string(), "rate limited"))
            }
        })
        .map(|x| x.to_string())
}

#[pyfunction]
fn check_bug_submit_url_canonical(url: &str, net_access: Option<bool>) -> PyResult<String> {
    let url =
        Url::parse(url).map_err(|e| PyRuntimeError::new_err(format!("Invalid URL: {}", e)))?;
    upstream_ontologist::check_bug_submit_url_canonical(&url, net_access)
        .map_err(|e| match e {
            CanonicalizeError::InvalidUrl(url, msg) => InvalidUrl::new_err((url.to_string(), msg)),
            CanonicalizeError::Unverifiable(url, msg) => {
                UnverifiableUrl::new_err((url.to_string(), msg))
            }
            CanonicalizeError::RateLimited(url) => {
                UnverifiableUrl::new_err((url.to_string(), "rate limited"))
            }
        })
        .map(|x| x.to_string())
}

#[pyfunction]
fn extract_sf_project_name(url: &str) -> Option<String> {
    upstream_ontologist::extract_sf_project_name(url)
}

#[pyfunction]
fn extract_pecl_package_name(url: &str) -> Option<String> {
    upstream_ontologist::extract_pecl_package_name(url)
}

#[pyfunction]
fn metadata_from_url(py: Python, url: &str, origin: Option<&str>) -> PyResult<Vec<PyObject>> {
    upstream_ontologist::metadata_from_url(url, origin)
        .into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn get_repology_metadata(py: Python, name: &str, distro: Option<&str>) -> PyResult<PyObject> {
    let ret = upstream_ontologist::get_repology_metadata(name, distro);

    if ret.is_none() {
        return Ok(py.None());
    }

    Ok(json_to_py(py, ret.unwrap())?)
}

#[pyfunction]
fn guess_from_security_md(
    py: Python,
    name: &str,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    upstream_ontologist::providers::security_md::guess_from_security_md(
        name,
        path.as_path(),
        trust_package,
    )
    .into_iter()
    .map(|x| upstream_datum_with_metadata_to_py(py, x))
    .collect()
}

#[pyfunction]
fn get_sf_metadata(project: &str) -> PyResult<PyObject> {
    if let Some(ret) = upstream_ontologist::get_sf_metadata(project) {
        Python::with_gil(|py| Ok(json_to_py(py, ret)?))
    } else {
        return Err(NoSuchForgeProject::new_err((project.to_string(),)));
    }
}

#[pyfunction]
fn known_bad_guess(py: Python, datum: PyObject) -> PyResult<bool> {
    let datum = py_to_upstream_datum(py, &datum)?;
    Ok(datum.known_bad_guess())
}

#[pyfunction]
fn guess_from_debian_patch(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    upstream_ontologist::providers::debian::guess_from_debian_patch(path.as_path(), trust_package)
        .map_err(map_provider_err_to_py_err)?
        .into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect()
}

#[pyfunction]
fn guess_from_path(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    upstream_ontologist::guess_from_path(path.as_path(), trust_package)
        .map_err(map_provider_err_to_py_err)?
        .into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect()
}

#[pyfunction(name = "skip_paragraph")]
fn readme_skip_paragraph(py: Python, para: &str) -> PyResult<(bool, Vec<PyObject>)> {
    let (skip, para) = upstream_ontologist::readme::skip_paragraph(para);
    let para = para
        .into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()?;
    Ok((skip, para))
}

#[pyfunction]
fn guess_from_pyproject_toml(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    upstream_ontologist::providers::python::guess_from_pyproject_toml(path.as_path(), trust_package)
        .map_err(map_provider_err_to_py_err)?
        .into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect()
}

#[pyfunction]
fn guess_from_setup_cfg(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    upstream_ontologist::providers::python::guess_from_setup_cfg(path.as_path(), trust_package)
        .map_err(map_provider_err_to_py_err)?
        .into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect()
}

#[pyfunction]
fn guess_from_setup_py(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    upstream_ontologist::providers::python::guess_from_setup_py(path.as_path(), trust_package)
        .map_err(map_provider_err_to_py_err)?
        .into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect()
}

#[pyfunction]
fn guess_from_package_yaml(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    upstream_ontologist::providers::package_yaml::guess_from_package_yaml(
        path.as_path(),
        trust_package,
    )
    .map_err(map_provider_err_to_py_err)?
    .into_iter()
    .map(|x| upstream_datum_with_metadata_to_py(py, x))
    .collect()
}

#[pyfunction]
fn guess_from_pkg_info(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    upstream_ontologist::providers::python::guess_from_pkg_info(path.as_path(), trust_package)
        .map_err(map_provider_err_to_py_err)?
        .into_iter()
        .map(|x| upstream_datum_with_metadata_to_py(py, x))
        .collect()
}

#[pymodule]
fn _upstream_ontologist(py: Python, m: &PyModule) -> PyResult<()> {
    pyo3_log::init();
    m.add_wrapped(wrap_pyfunction!(url_from_git_clone_command))?;
    m.add_wrapped(wrap_pyfunction!(url_from_fossil_clone_command))?;
    m.add_wrapped(wrap_pyfunction!(url_from_svn_co_command))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_meson))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_package_json))?;
    m.add_wrapped(wrap_pyfunction!(debian_is_native))?;
    m.add_wrapped(wrap_pyfunction!(drop_vcs_in_scheme))?;
    m.add_wrapped(wrap_pyfunction!(unsplit_vcs_url))?;
    m.add_wrapped(wrap_pyfunction!(load_json_url))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_composer_json))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_package_xml))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_dist_ini))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_perl_dist_name))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_perl_module))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_pod))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_pubspec_yaml))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_authors))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_metadata_json))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_meta_json))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_travis_yml))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_meta_yml))?;
    m.add_wrapped(wrap_pyfunction!(metadata_from_itp_bug_body))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_metainfo))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_doap))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_opam))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_pom_xml))?;
    m.add_wrapped(wrap_pyfunction!(plausible_vcs_url))?;
    m.add_wrapped(wrap_pyfunction!(plausible_vcs_browse_url))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_wscript))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_makefile_pl))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_go_mod))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_cabal))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_cabal_lines))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_git_config))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_aur))?;
    m.add_wrapped(wrap_pyfunction!(check_url_canonical))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_environment))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_nuspec))?;
    m.add_wrapped(wrap_pyfunction!(guess_repo_from_url))?;
    m.add_wrapped(wrap_pyfunction!(probe_gitlab_host))?;
    m.add_wrapped(wrap_pyfunction!(is_gitlab_site))?;
    m.add_wrapped(wrap_pyfunction!(check_repository_url_canonical))?;
    m.add_wrapped(wrap_pyfunction!(probe_upstream_branch_url))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_gemspec))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_launchpad))?;
    m.add_wrapped(wrap_pyfunction!(canonical_git_repo_url))?;
    m.add_wrapped(wrap_pyfunction!(browse_url_from_repo_url))?;
    m.add_wrapped(wrap_pyfunction!(find_public_repo_url))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_configure))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_r_description))?;
    m.add_wrapped(wrap_pyfunction!(find_forge))?;
    m.add_wrapped(wrap_pyfunction!(repo_url_from_merge_request_url))?;
    m.add_wrapped(wrap_pyfunction!(bug_database_from_issue_url))?;
    m.add_wrapped(wrap_pyfunction!(guess_bug_database_url_from_repo_url))?;
    m.add_wrapped(wrap_pyfunction!(bug_database_url_from_bug_submit_url))?;
    m.add_wrapped(wrap_pyfunction!(bug_submit_url_from_bug_database_url))?;
    m.add_wrapped(wrap_pyfunction!(check_bug_database_canonical))?;
    m.add_wrapped(wrap_pyfunction!(check_bug_submit_url_canonical))?;
    m.add_wrapped(wrap_pyfunction!(extract_sf_project_name))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_cargo))?;
    m.add_wrapped(wrap_pyfunction!(extract_pecl_package_name))?;
    m.add_wrapped(wrap_pyfunction!(metadata_from_url))?;
    m.add_wrapped(wrap_pyfunction!(get_repology_metadata))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_security_md))?;
    m.add_wrapped(wrap_pyfunction!(get_sf_metadata))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_debian_patch))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_path))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_pyproject_toml))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_setup_cfg))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_setup_py))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_package_yaml))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_pkg_info))?;
    m.add_class::<Forge>()?;
    m.add_class::<GitHub>()?;
    m.add_class::<GitLab>()?;
    m.add_class::<Launchpad>()?;
    m.add_class::<SourceForge>()?;
    m.add("InvalidUrl", py.get_type::<InvalidUrl>())?;
    m.add("UnverifiableUrl", py.get_type::<UnverifiableUrl>())?;
    m.add(
        "NoSuchRepologyProject",
        py.get_type::<NoSuchRepologyProject>(),
    )?;
    m.add("NoSuchForgeProject", py.get_type::<NoSuchForgeProject>())?;
    m.add_wrapped(wrap_pyfunction!(known_bad_guess))?;
    let readmem = PyModule::new(py, "readme")?;
    readmem.add_wrapped(wrap_pyfunction!(readme_skip_paragraph))?;
    m.add_submodule(readmem)?;
    m.add("ParseError", py.get_type::<ParseError>())?;
    Ok(())
}
