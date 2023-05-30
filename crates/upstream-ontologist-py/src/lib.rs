use pyo3::prelude::*;

#[pyfunction]
fn url_from_git_clone_command(command: &[u8]) -> Option<String> {
    upstream_ontologist::url_from_git_clone_command(command)
}

#[pymodule]
fn _upstream_ontologist(py: Python, m: &PyModule) -> PyResult<()> {
    m.add_wrapped(wrap_pyfunction!(url_from_git_clone_command))?;
    Ok(())
}
