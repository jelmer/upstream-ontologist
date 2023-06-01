use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::exceptions::PyRuntimeError;
use pyo3::import_exception;
use pyo3::prelude::*;
use std::path::PathBuf;
use upstream_ontologist::CanonicalizeError;

import_exception!(urllib.error, HTTPError);
create_exception!(upstream_ontologist, UnverifiableUrl, PyException);
create_exception!(upstream_ontologist, InvalidUrl, PyException);

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

fn upstream_datum_to_py(
    py: Python,
    datum: upstream_ontologist::UpstreamDatumWithMetadata,
) -> PyResult<PyObject> {
    let m = PyModule::import(py, "upstream_ontologist.guess")?;

    let UpstreamDatumCls = m.getattr("UpstreamDatum")?;

    let PersonCls = m.getattr("Person")?;

    {
        let datum = UpstreamDatumCls.call1((
            datum.datum.field(),
            match datum.datum {
                upstream_ontologist::UpstreamDatum::Name(n) => n.into_py(py),
                upstream_ontologist::UpstreamDatum::Version(v) => v.into_py(py),
                upstream_ontologist::UpstreamDatum::Contact(c) => c.into_py(py),
                upstream_ontologist::UpstreamDatum::Summary(s) => s.into_py(py),
                upstream_ontologist::UpstreamDatum::License(l) => l.into_py(py),
                upstream_ontologist::UpstreamDatum::Homepage(h) => h.into_py(py),
                upstream_ontologist::UpstreamDatum::Description(d) => d.into_py(py),
                upstream_ontologist::UpstreamDatum::BugDatabase(b) => b.into_py(py),
                upstream_ontologist::UpstreamDatum::BugSubmit(b) => b.into_py(py),
                upstream_ontologist::UpstreamDatum::Repository(r) => r.into_py(py),
                upstream_ontologist::UpstreamDatum::RepositoryBrowse(r) => r.into_py(py),
                upstream_ontologist::UpstreamDatum::SecurityMD(s) => s.into_py(py),
                upstream_ontologist::UpstreamDatum::SecurityContact(s) => s.into_py(py),
                upstream_ontologist::UpstreamDatum::CargoCrate(c) => c.into_py(py),
                upstream_ontologist::UpstreamDatum::Keywords(ks) => ks.into_py(py),
                upstream_ontologist::UpstreamDatum::Copyright(c) => c.into_py(py),
                upstream_ontologist::UpstreamDatum::Documentation(a) => a.into_py(py),
                upstream_ontologist::UpstreamDatum::GoImportPath(ip) => ip.into_py(py),
                upstream_ontologist::UpstreamDatum::Maintainer(m) => {
                    PersonCls.call1((m.name, m.email, m.url))?.into_py(py)
                }
                upstream_ontologist::UpstreamDatum::Author(a) => a
                    .into_iter()
                    .map(|x| PersonCls.call1((x.name, x.email, x.url)))
                    .collect::<PyResult<Vec<&PyAny>>>()?
                    .into_py(py),
                upstream_ontologist::UpstreamDatum::Wiki(w) => w.into_py(py),
                upstream_ontologist::UpstreamDatum::Download(d) => d.into_py(py),
                upstream_ontologist::UpstreamDatum::MailingList(m) => m.into_py(py),
            },
            datum.certainty.map(|x| x.to_string()),
            datum.origin,
        ))?;
        Ok(datum.to_object(py))
    }
}

#[pyfunction]
fn guess_from_meson(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_meson(path.as_path(), trust_package);

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_package_json(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_package_json(path.as_path(), trust_package);

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
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

#[pyfunction]
fn load_json_url(http_url: &str, timeout: Option<u64>) -> PyResult<String> {
    let http_url = http_url
        .parse::<reqwest::Url>()
        .map_err(|e| PyRuntimeError::new_err(format!("{}: {}", e, http_url)))?;
    Ok(
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
            })?
            .to_string(),
    )
}

#[pyfunction]
fn guess_from_composer_json(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_composer_json(path.as_path(), trust_package);

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_package_xml(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_package_xml(path.as_path(), trust_package);

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_dist_ini(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_dist_ini(path.as_path(), trust_package);

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_perl_dist_name(
    py: Python,
    path: PathBuf,
    dist_name: &str,
) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_perl_dist_name(path.as_path(), dist_name);

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_perl_module(py: Python, path: PathBuf) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_perl_module(path.as_path());

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_pod(py: Python, contents: &str) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_pod(contents);

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_pubspec_yaml(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_pubspec_yaml(path.as_path(), trust_package);

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_authors(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_authors(path.as_path(), trust_package);

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_metadata_json(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_metadata_json(path.as_path(), trust_package);

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_meta_json(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_meta_json(path.as_path(), trust_package);

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
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

#[pyfunction]
fn guess_from_travis_yml(
    py: Python,
    path: PathBuf,
    trust_package: bool,
) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_travis_yml(path.as_path(), trust_package);

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_meta_yml(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_meta_yml(path.as_path(), trust_package);

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn metadata_from_itp_bug_body(py: Python, body: &str) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::metadata_from_itp_bug_body(body);

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_metainfo(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_metainfo(path.as_path(), trust_package);

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
}

#[pyfunction]
fn guess_from_doap(py: Python, path: PathBuf, trust_package: bool) -> PyResult<Vec<PyObject>> {
    let ret = upstream_ontologist::guess_from_doap(path.as_path(), trust_package);

    ret.into_iter()
        .map(|x| upstream_datum_to_py(py, x))
        .collect::<PyResult<Vec<PyObject>>>()
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
    m.add_class::<Forge>()?;
    m.add_class::<GitHub>()?;
    m.add_class::<GitLab>()?;
    m.add("InvalidUrl", py.get_type::<InvalidUrl>())?;
    m.add("UnverifiableUrl", py.get_type::<UnverifiableUrl>())?;
    Ok(())
}