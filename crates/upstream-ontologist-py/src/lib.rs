use pyo3::exceptions::PyRuntimeError;
use pyo3::import_exception;
use pyo3::prelude::*;
use std::path::PathBuf;

import_exception!(urllib.error, HTTPError);

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
                upstream_ontologist::UpstreamDatum::Maintainer(m) => {
                    PersonCls.call1((m.name, m.email, m.url))?.into_py(py)
                }
                upstream_ontologist::UpstreamDatum::Author(a) => a
                    .into_iter()
                    .map(|x| PersonCls.call1((x.name, x.email, x.url)))
                    .collect::<PyResult<Vec<&PyAny>>>()?
                    .into_py(py),
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
    Ok(
        upstream_ontologist::load_json_url(http_url, timeout.map(std::time::Duration::from_secs))
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

#[pymodule]
fn _upstream_ontologist(py: Python, m: &PyModule) -> PyResult<()> {
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
    Ok(())
}
