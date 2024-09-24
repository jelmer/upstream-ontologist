use pyo3::exceptions::{PyKeyError, PyRuntimeError, PyStopIteration, PyValueError};
use pyo3::import_exception;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple, PyType};
use std::str::FromStr;
use upstream_ontologist::{Certainty, Origin};
use url::Url;

import_exception!(urllib.error, HTTPError);

#[pyfunction]
fn drop_vcs_in_scheme(url: &str) -> String {
    upstream_ontologist::vcs::drop_vcs_in_scheme(&url.parse().unwrap())
        .map_or_else(|| url.to_string(), |u| u.to_string())
}

#[pyfunction]
#[pyo3(signature = (url, net_access=None))]
fn canonical_git_repo_url(url: &str, net_access: Option<bool>) -> PyResult<String> {
    let url =
        Url::parse(url).map_err(|e| PyRuntimeError::new_err(format!("Invalid URL: {}", e)))?;
    Ok(
        upstream_ontologist::vcs::canonical_git_repo_url(&url, net_access)
            .map_or_else(|| url.to_string(), |u| u.to_string()),
    )
}

#[pyfunction]
#[pyo3(signature = (url, net_access=None))]
fn find_public_repo_url(url: &str, net_access: Option<bool>) -> PyResult<Option<String>> {
    Ok(upstream_ontologist::vcs::find_public_repo_url(
        url, net_access,
    ))
}

#[pyfunction]
fn known_bad_guess(py: Python, datum: PyObject) -> PyResult<bool> {
    let datum: upstream_ontologist::UpstreamDatum = datum.extract(py)?;
    Ok(datum.known_bad_guess())
}

#[pyfunction]
fn fixup_rcp_style_git_repo_url(url: &str) -> PyResult<String> {
    Ok(upstream_ontologist::vcs::fixup_rcp_style_git_repo_url(url)
        .map_or(url.to_string(), |u| u.to_string()))
}

#[pyfunction]
#[pyo3(signature = (url, branch=None, net_access=None))]
pub fn find_secure_repo_url(
    url: String,
    branch: Option<&str>,
    net_access: Option<bool>,
) -> Option<String> {
    upstream_ontologist::vcs::find_secure_repo_url(url.parse().unwrap(), branch, net_access)
        .map(|u| u.to_string())
}

#[pyfunction]
fn convert_cvs_list_to_str(urls: Vec<String>) -> Option<String> {
    let urls = urls.iter().map(|x| x.as_str()).collect::<Vec<&str>>();
    upstream_ontologist::vcs::convert_cvs_list_to_str(urls.as_slice())
}

#[pyfunction]
#[pyo3(signature = (location, branch=None, subpath=None))]
fn fixup_broken_git_details(
    location: &str,
    branch: Option<&str>,
    subpath: Option<&str>,
) -> (String, Option<String>, Option<String>) {
    let url = upstream_ontologist::vcs::fixup_git_url(location);
    let location = upstream_ontologist::vcs::VcsLocation {
        url: url.parse().unwrap(),
        branch: branch.map(|s| s.to_string()),
        subpath: subpath.map(|s| s.to_string()),
    };
    let ret = upstream_ontologist::vcs::fixup_git_location(&location);
    (
        ret.url.to_string(),
        ret.branch.as_ref().map(|s| s.to_string()),
        ret.subpath.as_ref().map(|s| s.to_string()),
    )
}

fn extract_str_value(py: Python, value: PyObject) -> PyResult<String> {
    let value = value.extract::<PyObject>(py)?;

    value.extract::<String>(py)
}

#[derive(Clone)]
#[pyclass]
struct UpstreamDatum(pub(crate) upstream_ontologist::UpstreamDatumWithMetadata);

#[pymethods]
impl UpstreamDatum {
    #[new]
    #[pyo3(signature = (field, value, certainty=None, origin=None))]
    fn new(
        py: Python,
        field: String,
        value: PyObject,
        certainty: Option<String>,
        origin: Option<Origin>,
    ) -> PyResult<Self> {
        Ok(UpstreamDatum(
            upstream_ontologist::UpstreamDatumWithMetadata {
                datum: match field.as_str() {
                    "Name" => {
                        upstream_ontologist::UpstreamDatum::Name(extract_str_value(py, value)?)
                    }
                    "Version" => {
                        upstream_ontologist::UpstreamDatum::Version(extract_str_value(py, value)?)
                    }
                    "Summary" => {
                        upstream_ontologist::UpstreamDatum::Summary(extract_str_value(py, value)?)
                    }
                    "Description" => upstream_ontologist::UpstreamDatum::Description(
                        extract_str_value(py, value)?,
                    ),
                    "Homepage" => {
                        upstream_ontologist::UpstreamDatum::Homepage(extract_str_value(py, value)?)
                    }
                    "Repository" => {
                        // Check if the value is a list rather than a string
                        if let Ok(value) = value.extract::<Vec<String>>(py) {
                            upstream_ontologist::UpstreamDatum::Repository(value.join(" "))
                        } else {
                            upstream_ontologist::UpstreamDatum::Repository(extract_str_value(
                                py, value,
                            )?)
                        }
                    }
                    "Repository-Browse" => upstream_ontologist::UpstreamDatum::RepositoryBrowse(
                        extract_str_value(py, value)?,
                    ),
                    "License" => {
                        upstream_ontologist::UpstreamDatum::License(extract_str_value(py, value)?)
                    }
                    "Author" => {
                        upstream_ontologist::UpstreamDatum::Author(value.extract(py).unwrap())
                    }
                    "Bug-Database" => upstream_ontologist::UpstreamDatum::BugDatabase(
                        extract_str_value(py, value)?,
                    ),
                    "Bug-Submit" => {
                        upstream_ontologist::UpstreamDatum::BugSubmit(extract_str_value(py, value)?)
                    }
                    "Contact" => {
                        upstream_ontologist::UpstreamDatum::Contact(extract_str_value(py, value)?)
                    }
                    "Cargo-Crate" => upstream_ontologist::UpstreamDatum::CargoCrate(
                        extract_str_value(py, value)?,
                    ),
                    "Security-MD" => upstream_ontologist::UpstreamDatum::SecurityMD(
                        extract_str_value(py, value)?,
                    ),
                    "Keywords" => {
                        upstream_ontologist::UpstreamDatum::Keywords(value.extract(py).unwrap())
                    }
                    "Maintainer" => {
                        upstream_ontologist::UpstreamDatum::Maintainer(value.extract(py).unwrap())
                    }
                    "Copyright" => {
                        upstream_ontologist::UpstreamDatum::Copyright(value.extract(py).unwrap())
                    }
                    "Documentation" => upstream_ontologist::UpstreamDatum::Documentation(
                        value.extract(py).unwrap(),
                    ),
                    "Go-Import-Path" => {
                        upstream_ontologist::UpstreamDatum::GoImportPath(value.extract(py).unwrap())
                    }
                    "Download" => {
                        upstream_ontologist::UpstreamDatum::Download(value.extract(py).unwrap())
                    }
                    "Wiki" => upstream_ontologist::UpstreamDatum::Wiki(value.extract(py).unwrap()),
                    "MailingList" => {
                        upstream_ontologist::UpstreamDatum::MailingList(value.extract(py).unwrap())
                    }
                    "SourceForge-Project" => {
                        upstream_ontologist::UpstreamDatum::SourceForgeProject(
                            value.extract(py).unwrap(),
                        )
                    }
                    "Archive" => {
                        upstream_ontologist::UpstreamDatum::Archive(value.extract(py).unwrap())
                    }
                    "Demo" => upstream_ontologist::UpstreamDatum::Demo(value.extract(py).unwrap()),
                    "Pecl-Package" => {
                        upstream_ontologist::UpstreamDatum::PeclPackage(value.extract(py).unwrap())
                    }
                    "Haskell-Package" => upstream_ontologist::UpstreamDatum::HaskellPackage(
                        value.extract(py).unwrap(),
                    ),
                    "Funding" => {
                        upstream_ontologist::UpstreamDatum::Funding(value.extract(py).unwrap())
                    }
                    "Changelog" => {
                        upstream_ontologist::UpstreamDatum::Changelog(value.extract(py).unwrap())
                    }
                    "Debian-ITP" => {
                        upstream_ontologist::UpstreamDatum::DebianITP(value.extract(py).unwrap())
                    }
                    "Screenshots" => {
                        upstream_ontologist::UpstreamDatum::Screenshots(value.extract(py).unwrap())
                    }
                    "Cite-As" => {
                        upstream_ontologist::UpstreamDatum::CiteAs(value.extract(py).unwrap())
                    }
                    "Registry" => {
                        upstream_ontologist::UpstreamDatum::Registry(value.extract(py).unwrap())
                    }
                    "Donation" => {
                        upstream_ontologist::UpstreamDatum::Donation(value.extract(py).unwrap())
                    }
                    "Webservice" => {
                        upstream_ontologist::UpstreamDatum::Webservice(value.extract(py).unwrap())
                    }
                    _ => {
                        return Err(PyValueError::new_err(format!("Unknown field: {}", field)));
                    }
                },
                origin,
                certainty: certainty.map(|s| Certainty::from_str(&s).unwrap()),
            },
        ))
    }

    #[getter]
    fn field(&self) -> PyResult<String> {
        Ok(self.0.datum.field().to_string())
    }

    #[getter]
    fn value(&self, py: Python) -> PyResult<PyObject> {
        let value = self
            .0
            .datum
            .to_object(py)
            .extract::<(String, PyObject)>(py)
            .unwrap()
            .1;
        assert!(!value.bind(py).is_instance_of::<PyTuple>());
        Ok(value)
    }

    #[getter]
    fn origin(&self) -> Option<Origin> {
        self.0.origin.clone()
    }

    #[setter]
    fn set_origin(&mut self, origin: Option<Origin>) {
        self.0.origin = origin;
    }

    #[getter]
    fn certainty(&self) -> Option<String> {
        self.0.certainty.map(|c| c.to_string())
    }

    #[setter]
    pub fn set_certainty(&mut self, certainty: Option<String>) {
        self.0.certainty = certainty.map(|s| Certainty::from_str(&s).unwrap());
    }

    fn __eq__(lhs: &Bound<Self>, rhs: &Bound<Self>) -> PyResult<bool> {
        Ok(lhs.borrow().0 == rhs.borrow().0)
    }

    fn __ne__(lhs: &Bound<Self>, rhs: &Bound<Self>) -> PyResult<bool> {
        Ok(lhs.borrow().0 != rhs.borrow().0)
    }

    fn __str__(&self) -> PyResult<String> {
        Ok(format!("{}: {}", self.0.datum.field(), self.0.datum))
    }

    fn __repr__(slf: PyRef<Self>) -> PyResult<String> {
        Ok(format!(
            "UpstreamDatum({}, {}, {}, certainty={})",
            slf.0.datum.field(),
            slf.0.datum,
            slf.0
                .origin
                .as_ref()
                .map(|s| format!("Some({})", s))
                .unwrap_or_else(|| "None".to_string()),
            slf.0
                .certainty
                .as_ref()
                .map(|c| format!("Some({})", c))
                .unwrap_or_else(|| "None".to_string()),
        ))
    }
}

#[pyclass]
struct UpstreamMetadata(pub(crate) upstream_ontologist::UpstreamMetadata);

#[allow(non_snake_case)]
#[pymethods]
impl UpstreamMetadata {
    fn __getitem__(&self, field: &str) -> PyResult<UpstreamDatum> {
        self.0
            .get(field)
            .map(|datum| UpstreamDatum(datum.clone()))
            .ok_or_else(|| PyKeyError::new_err(format!("No such field: {}", field)))
    }

    fn __delitem__(&mut self, field: &str) -> PyResult<()> {
        self.0.remove(field);
        Ok(())
    }

    fn __contains__(&self, field: &str) -> bool {
        self.0.contains_key(field)
    }

    pub fn items(&self) -> Vec<(String, UpstreamDatum)> {
        self.0
            .iter()
            .map(|datum| {
                (
                    datum.datum.field().to_string(),
                    UpstreamDatum(datum.clone()),
                )
            })
            .collect()
    }

    pub fn values(&self) -> Vec<UpstreamDatum> {
        self.0
            .iter()
            .map(|datum| UpstreamDatum(datum.clone()))
            .collect()
    }

    #[pyo3(signature = (field, default=None))]
    pub fn get(&self, py: Python, field: &str, default: Option<PyObject>) -> PyObject {
        let default = default.unwrap_or_else(|| py.None());
        let value = self
            .0
            .get(field)
            .map(|datum| UpstreamDatum(datum.clone()).into_py(py));

        value.unwrap_or(default)
    }

    fn __setitem__(&mut self, field: &str, datum: UpstreamDatum) -> PyResult<()> {
        assert_eq!(field, datum.0.datum.field());
        self.0.insert(datum.0);
        Ok(())
    }

    #[new]
    #[pyo3(signature = (**kwargs))]
    fn new(kwargs: Option<Bound<PyDict>>) -> Self {
        let mut ret = UpstreamMetadata(upstream_ontologist::UpstreamMetadata::new());

        if let Some(kwargs) = kwargs {
            for item in kwargs.items() {
                let datum = item.extract::<UpstreamDatum>().unwrap();
                ret.0.insert(datum.0);
            }
        }

        ret
    }

    #[classmethod]
    #[pyo3(signature = (d, default_certainty=None))]
    pub fn from_dict(
        _cls: &Bound<PyType>,
        py: Python,
        d: &Bound<PyDict>,
        default_certainty: Option<Certainty>,
    ) -> PyResult<Self> {
        let mut data = Vec::new();
        let di = d.iter();
        for t in di {
            let t = t.to_object(py);
            let mut datum: upstream_ontologist::UpstreamDatumWithMetadata =
                if let Ok(wm) = t.extract(py) {
                    wm
                } else {
                    let wm: upstream_ontologist::UpstreamDatum = t.extract(py)?;

                    upstream_ontologist::UpstreamDatumWithMetadata {
                        datum: wm,
                        certainty: default_certainty,
                        origin: None,
                    }
                };

            if datum.certainty.is_none() {
                datum.certainty = default_certainty;
            }
            data.push(datum);
        }
        Ok(Self(upstream_ontologist::UpstreamMetadata::from_data(data)))
    }

    pub fn __iter__(slf: PyRef<Self>) -> PyResult<PyObject> {
        #[pyclass]
        struct UpstreamDatumIter {
            inner: Vec<upstream_ontologist::UpstreamDatumWithMetadata>,
        }
        #[pymethods]
        impl UpstreamDatumIter {
            fn __next__(&mut self) -> Option<UpstreamDatum> {
                self.inner.pop().map(UpstreamDatum)
            }
        }
        Ok(UpstreamDatumIter {
            inner: slf.0.iter().cloned().collect::<Vec<_>>(),
        }
        .into_py(slf.py()))
    }
}

#[pyfunction]
fn check_upstream_metadata(metadata: &mut UpstreamMetadata) -> PyResult<()> {
    upstream_ontologist::check_upstream_metadata(&mut metadata.0, None);
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (metadata, path, minimum_certainty=None, net_access=None, consult_external_directory=None))]
fn extend_upstream_metadata(
    metadata: &mut UpstreamMetadata,
    path: std::path::PathBuf,
    minimum_certainty: Option<String>,
    net_access: Option<bool>,
    consult_external_directory: Option<bool>,
) -> PyResult<()> {
    let minimum_certainty = minimum_certainty
        .map(|s| s.parse())
        .transpose()
        .map_err(|e: String| PyValueError::new_err(format!("Invalid minimum_certainty: {}", e)))?;
    upstream_ontologist::extend_upstream_metadata(
        &mut metadata.0,
        path.as_path(),
        minimum_certainty,
        net_access,
        consult_external_directory,
    )?;
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (path, trust_package=None, net_access=None, consult_external_directory=None, check=None))]
fn guess_upstream_metadata(
    path: std::path::PathBuf,
    trust_package: Option<bool>,
    net_access: Option<bool>,
    consult_external_directory: Option<bool>,
    check: Option<bool>,
) -> PyResult<UpstreamMetadata> {
    Ok(UpstreamMetadata(
        upstream_ontologist::guess_upstream_metadata(
            path.as_path(),
            trust_package,
            net_access,
            consult_external_directory,
            check,
        )?,
    ))
}

#[pyfunction]
#[pyo3(signature = (path, trust_package=None, minimum_certainty=None))]
fn guess_upstream_metadata_items(
    py: Python,
    path: std::path::PathBuf,
    trust_package: Option<bool>,
    minimum_certainty: Option<String>,
) -> PyResult<Vec<PyObject>> {
    let metadata = upstream_ontologist::guess_upstream_metadata_items(
        path.as_path(),
        trust_package,
        minimum_certainty
            .map(|s| s.parse())
            .transpose()
            .map_err(|e: String| {
                PyValueError::new_err(format!("Invalid minimum_certainty: {}", e))
            })?,
    );
    Ok(metadata
        .into_iter()
        .map(|datum| datum.map(|o| o.to_object(py)))
        .filter_map(Result::ok)
        .collect::<Vec<PyObject>>())
}

#[pyfunction]
fn fix_upstream_metadata(metadata: &mut UpstreamMetadata) -> PyResult<()> {
    upstream_ontologist::fix_upstream_metadata(&mut metadata.0);
    Ok(())
}

#[pyfunction]
fn update_from_guesses(
    py: Python,
    metadata: &mut UpstreamMetadata,
    items_iter: PyObject,
) -> PyResult<Vec<UpstreamDatum>> {
    let mut items = vec![];
    loop {
        let item = match items_iter.call_method0(py, "__next__") {
            Ok(item) => item,
            Err(e) => {
                if e.is_instance_of::<PyStopIteration>(py) {
                    break;
                }
                return Err(e);
            }
        };
        items.push(item.extract::<UpstreamDatum>(py)?);
    }
    Ok(upstream_ontologist::update_from_guesses(
        metadata.0.mut_items(),
        items.into_iter().map(|datum| datum.0),
    )
    .into_iter()
    .map(UpstreamDatum)
    .collect())
}

#[pymodule]
fn _upstream_ontologist(m: &Bound<PyModule>) -> PyResult<()> {
    pyo3_log::init();
    m.add_wrapped(wrap_pyfunction!(drop_vcs_in_scheme))?;
    m.add_wrapped(wrap_pyfunction!(canonical_git_repo_url))?;
    m.add_wrapped(wrap_pyfunction!(find_public_repo_url))?;
    m.add_wrapped(wrap_pyfunction!(fixup_rcp_style_git_repo_url))?;
    m.add_wrapped(wrap_pyfunction!(check_upstream_metadata))?;
    m.add_wrapped(wrap_pyfunction!(extend_upstream_metadata))?;
    m.add_wrapped(wrap_pyfunction!(guess_upstream_metadata))?;
    m.add_wrapped(wrap_pyfunction!(fix_upstream_metadata))?;
    m.add_wrapped(wrap_pyfunction!(guess_upstream_metadata_items))?;
    m.add_wrapped(wrap_pyfunction!(update_from_guesses))?;
    m.add_wrapped(wrap_pyfunction!(find_secure_repo_url))?;
    m.add_wrapped(wrap_pyfunction!(convert_cvs_list_to_str))?;
    m.add_wrapped(wrap_pyfunction!(fixup_broken_git_details))?;
    m.add_class::<UpstreamMetadata>()?;
    m.add_class::<UpstreamDatum>()?;
    m.add_wrapped(wrap_pyfunction!(known_bad_guess))?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
