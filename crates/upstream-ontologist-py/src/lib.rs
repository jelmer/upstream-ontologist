use pyo3::prelude::*;
use std::path::PathBuf;

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
fn debian_is_native(path: PathBuf) -> PyResult<Option<bool>> {
    Ok(upstream_ontologist::debian_is_native(path.as_path())?)
}

#[pymodule]
fn _upstream_ontologist(py: Python, m: &PyModule) -> PyResult<()> {
    m.add_wrapped(wrap_pyfunction!(url_from_git_clone_command))?;
    m.add_wrapped(wrap_pyfunction!(url_from_fossil_clone_command))?;
    m.add_wrapped(wrap_pyfunction!(url_from_svn_co_command))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_meson))?;
    m.add_wrapped(wrap_pyfunction!(debian_is_native))?;
    m.add_wrapped(wrap_pyfunction!(drop_vcs_in_scheme))?;
    Ok(())
}
