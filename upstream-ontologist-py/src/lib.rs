use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyKeyError, PyRuntimeError, PyValueError, PyStopIteration};
use pyo3::import_exception;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pyo3::types::PyTuple;
use std::str::FromStr;
use upstream_ontologist::{CanonicalizeError, Origin, UpstreamPackage};
use url::Url;

import_exception!(urllib.error, HTTPError);
create_exception!(upstream_ontologist, UnverifiableUrl, PyException);
create_exception!(upstream_ontologist, InvalidUrl, PyException);
create_exception!(upstream_ontologist, NoSuchForgeProject, PyException);

#[pyfunction]
fn url_from_git_clone_command(command: &[u8]) -> Option<String> {
    upstream_ontologist::vcs_command::url_from_git_clone_command(command)
}

#[pyfunction]
fn url_from_fossil_clone_command(command: &[u8]) -> Option<String> {
    upstream_ontologist::vcs_command::url_from_fossil_clone_command(command)
}

#[pyfunction]
fn url_from_svn_co_command(command: &[u8]) -> Option<String> {
    upstream_ontologist::vcs_command::url_from_svn_co_command(command)
}

#[pyfunction]
fn url_from_cvs_co_command(command: &[u8]) -> Option<String> {
    upstream_ontologist::vcs_command::url_from_cvs_co_command(command)
}

#[pyfunction]
fn url_from_vcs_command(command: &[u8]) -> Option<String> {
    upstream_ontologist::vcs_command::url_from_vcs_command(command)
}

#[pyfunction]
fn drop_vcs_in_scheme(url: &str) -> String {
    upstream_ontologist::vcs::drop_vcs_in_scheme(&url.parse().unwrap())
        .map_or_else(|| url.to_string(), |u| u.to_string())
}

#[pyfunction]
fn unsplit_vcs_url(
    repo_url: &str,
    branch: Option<&str>,
    subpath: Option<&str>,
) -> PyResult<String> {
    let location = upstream_ontologist::vcs::VcsLocation {
        url: repo_url
            .parse()
            .map_err(|e: url::ParseError| PyValueError::new_err(e.to_string()))?,
        branch: branch.map(|b| b.to_string()),
        subpath: subpath.map(|b| b.to_string()),
    };
    Ok(upstream_ontologist::vcs::unsplit_vcs_url(&location))
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
fn plausible_vcs_url(url: &str) -> PyResult<bool> {
    Ok(upstream_ontologist::vcs::plausible_url(url))
}

#[pyfunction]
fn plausible_vcs_browse_url(url: &str) -> PyResult<bool> {
    Ok(upstream_ontologist::vcs::plausible_browse_url(url))
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
fn guess_repo_from_url(url: &str, net_access: Option<bool>) -> PyResult<Option<String>> {
    if let Ok(url) = Url::parse(url) {
        Ok(upstream_ontologist::vcs::guess_repo_from_url(
            &url, net_access,
        ))
    } else {
        Ok(None)
    }
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
fn guess_from_launchpad(
    py: Python,
    package: &str,
    distribution: Option<&str>,
    suite: Option<&str>,
) -> PyResult<PyObject> {
    let ret = upstream_ontologist::providers::launchpad::guess_from_launchpad(
        package,
        distribution,
        suite,
    );

    if ret.is_none() {
        Ok(Vec::<String>::new().to_object(py))
    } else {
        Ok(ret.to_object(py))
    }
}

#[pyfunction]
fn browse_url_from_repo_url(
    url: &str,
    branch: Option<&str>,
    subpath: Option<&str>,
    net_access: Option<bool>,
) -> PyResult<Option<String>> {
    let location = upstream_ontologist::vcs::VcsLocation {
        url: Url::parse(url).map_err(|e| PyValueError::new_err(format!("Invalid URL: {}", e)))?,
        branch: branch.map(|s| s.to_string()),
        subpath: subpath.map(|s| s.to_string()),
    };
    Ok(
        upstream_ontologist::vcs::browse_url_from_repo_url(&location, net_access)
            .map(|u| u.to_string()),
    )
}

#[pyfunction]
fn canonical_git_repo_url(url: &str, net_access: Option<bool>) -> PyResult<String> {
    let url =
        Url::parse(url).map_err(|e| PyRuntimeError::new_err(format!("Invalid URL: {}", e)))?;
    Ok(
        upstream_ontologist::vcs::canonical_git_repo_url(&url, net_access)
            .map_or_else(|| url.to_string(), |u| u.to_string()),
    )
}

#[pyfunction]
fn find_public_repo_url(url: &str, net_access: Option<bool>) -> PyResult<Option<String>> {
    Ok(upstream_ontologist::vcs::find_public_repo_url(
        url, net_access,
    ))
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
fn known_bad_guess(py: Python, datum: PyObject) -> PyResult<bool> {
    let datum: upstream_ontologist::UpstreamDatum = datum.extract(py)?;
    Ok(datum.known_bad_guess())
}

#[pyfunction(name = "skip_paragraph")]
fn readme_skip_paragraph(py: Python, para: &str) -> PyResult<(bool, PyObject)> {
    let (skip, para) = upstream_ontologist::readme::skip_paragraph(para);
    Ok((skip, para.to_object(py)))
}

#[pyfunction]
fn fixup_rcp_style_git_repo_url(url: &str) -> PyResult<String> {
    Ok(upstream_ontologist::vcs::fixup_rcp_style_git_repo_url(url)
        .map_or(url.to_string(), |u| u.to_string()))
}

#[pyfunction]
fn valid_debian_package_name(name: &str) -> PyResult<bool> {
    Ok(upstream_ontologist::debian::valid_debian_package_name(name))
}

#[pyfunction]
fn debian_to_upstream_version(version: &str) -> PyResult<String> {
    Ok(upstream_ontologist::debian::debian_to_upstream_version(version).to_string())
}

#[pyfunction]
fn upstream_name_to_debian_source_name(name: &str) -> PyResult<String> {
    Ok(upstream_ontologist::debian::upstream_name_to_debian_source_name(name))
}

#[pyfunction]
fn upstream_package_to_debian_binary_name(package: UpstreamPackage) -> PyResult<String> {
    Ok(upstream_ontologist::debian::upstream_package_to_debian_binary_name(&package))
}

#[pyfunction]
fn upstream_package_to_debian_source_name(package: UpstreamPackage) -> PyResult<String> {
    Ok(upstream_ontologist::debian::upstream_package_to_debian_source_name(&package))
}

#[pyfunction]
pub fn find_secure_repo_url(
    url: String,
    branch: Option<&str>,
    net_access: Option<bool>,
) -> Option<String> {
    upstream_ontologist::vcs::find_secure_repo_url(url.parse().unwrap(), branch, net_access)
        .map(|u| u.to_string())
}

#[pyfunction]
fn sanitize_url(url: &str) -> PyResult<String> {
    let url: url::Url = url
        .parse()
        .map_err(|e| PyValueError::new_err(format!("{}", e)))?;
    Ok(upstream_ontologist::vcs::sanitize_url(&url).to_string())
}

#[pyfunction]
fn convert_cvs_list_to_str(urls: Vec<&str>) -> Option<String> {
    upstream_ontologist::vcs::convert_cvs_list_to_str(urls.as_slice())
}

#[pyfunction]
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

#[derive(Clone)]
#[pyclass]
struct UpstreamDatum(pub(crate) upstream_ontologist::UpstreamDatumWithMetadata);

#[pymethods]
impl UpstreamDatum {
    #[new]
    fn new(
        py: Python,
        field: String,
        value: PyObject,
        certainty: Option<String>,
        origin: Option<Origin>,
    ) -> Self {
        UpstreamDatum(upstream_ontologist::UpstreamDatumWithMetadata {
            datum: match field.as_str() {
                "Name" => upstream_ontologist::UpstreamDatum::Name(value.extract(py).unwrap()),
                "Version" => {
                    upstream_ontologist::UpstreamDatum::Version(value.extract(py).unwrap())
                }
                "Summary" => {
                    upstream_ontologist::UpstreamDatum::Summary(value.extract(py).unwrap())
                }
                "Description" => {
                    upstream_ontologist::UpstreamDatum::Description(value.extract(py).unwrap())
                }
                "Homepage" => {
                    upstream_ontologist::UpstreamDatum::Homepage(value.extract(py).unwrap())
                }
                "Repository" => {
                    upstream_ontologist::UpstreamDatum::Repository(value.extract(py).unwrap())
                }
                "Repository-Browse" => {
                    upstream_ontologist::UpstreamDatum::RepositoryBrowse(value.extract(py).unwrap())
                }
                "License" => {
                    upstream_ontologist::UpstreamDatum::License(value.extract(py).unwrap())
                }
                "Author" => upstream_ontologist::UpstreamDatum::Author(value.extract(py).unwrap()),
                "Bug-Database" => {
                    upstream_ontologist::UpstreamDatum::BugDatabase(value.extract(py).unwrap())
                }
                "Bug-Submit" => {
                    upstream_ontologist::UpstreamDatum::BugSubmit(value.extract(py).unwrap())
                }
                "Contact" => {
                    upstream_ontologist::UpstreamDatum::Contact(value.extract(py).unwrap())
                }
                "Cargo-Crate" => {
                    upstream_ontologist::UpstreamDatum::CargoCrate(value.extract(py).unwrap())
                }
                "Security-MD" => {
                    upstream_ontologist::UpstreamDatum::SecurityMD(value.extract(py).unwrap())
                }
                "Keywords" => {
                    upstream_ontologist::UpstreamDatum::Keywords(value.extract(py).unwrap())
                }
                "Maintainer" => {
                    upstream_ontologist::UpstreamDatum::Maintainer(value.extract(py).unwrap())
                }
                "Copyright" => {
                    upstream_ontologist::UpstreamDatum::Copyright(value.extract(py).unwrap())
                }
                "Documentation" => {
                    upstream_ontologist::UpstreamDatum::Documentation(value.extract(py).unwrap())
                }
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
                "SourceForge-Project" => upstream_ontologist::UpstreamDatum::SourceForgeProject(
                    value.extract(py).unwrap(),
                ),
                "Archive" => {
                    upstream_ontologist::UpstreamDatum::Archive(value.extract(py).unwrap())
                }
                "Demo" => upstream_ontologist::UpstreamDatum::Demo(value.extract(py).unwrap()),
                "Pecl-Package" => {
                    upstream_ontologist::UpstreamDatum::PeclPackage(value.extract(py).unwrap())
                }
                "Haskell-Package" => {
                    upstream_ontologist::UpstreamDatum::HaskellPackage(value.extract(py).unwrap())
                }
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
                _ => panic!("Unknown field: {}", field),
            },
            origin,
            certainty: certainty.map(|s| upstream_ontologist::Certainty::from_str(&s).unwrap()),
        })
    }

    #[getter]
    fn field(&self) -> PyResult<String> {
        Ok(self.0.datum.field().to_string())
    }

    #[getter]
    fn value(&self, py: Python) -> PyResult<PyObject> {
        let value = self.0.datum.to_object(py).extract::<(String, PyObject)>(py).unwrap().1;
        assert!(!value.as_ref(py).is_instance_of::<PyTuple>());
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
        self.0.certainty = certainty.map(|s| upstream_ontologist::Certainty::from_str(&s).unwrap());
    }

    fn __eq__(lhs: &PyCell<Self>, rhs: &PyCell<Self>) -> PyResult<bool> {
        Ok(lhs.borrow().0 == rhs.borrow().0)
    }

    fn __ne__(lhs: &PyCell<Self>, rhs: &PyCell<Self>) -> PyResult<bool> {
        Ok(lhs.borrow().0 != rhs.borrow().0)
    }

    fn __str__(&self) -> PyResult<String> {
        Ok(format!(
            "{}: {}",
            self.0.datum.field(),
            self.0.datum.to_string()
        ))
    }

    fn __repr__(slf: PyRef<Self>) -> PyResult<String> {
        Ok(format!(
            "UpstreamDatum({}, {}, {}, certainty={})",
            slf.0.datum.field(),
            slf.0.datum.to_string(),
            slf.0
                .origin
                .as_ref()
                .map(|s| format!("Some({})", s))
                .unwrap_or_else(|| "None".to_string()),
            slf.0
                .certainty
                .as_ref()
                .map(|c| format!("Some({})", c.to_string()))
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
            .get(&field)
            .map(|datum| UpstreamDatum(datum.clone()))
            .ok_or_else(|| PyKeyError::new_err(format!("No such field: {}", field)))
    }

    fn __delitem__(&mut self, field: &str) -> PyResult<()> {
        self.0.remove(&field);
        Ok(())
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
        self.0.iter().map(|datum| UpstreamDatum(datum.clone())).collect()
    }

    pub fn get(&self, py: Python, field: &str, default: Option<PyObject>) -> PyObject {
        let default = default.unwrap_or_else(|| py.None());
        let value = self
            .0
            .get(&field)
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
    fn new(kwargs: Option<&PyDict>) -> Self {
        let mut ret = UpstreamMetadata(upstream_ontologist::UpstreamMetadata::new());

        if let Some(kwargs) = kwargs {
            for item in kwargs.items() {
                let datum = item.extract::<UpstreamDatum>().unwrap();
                ret.0.insert(datum.0);
            }
        }

        ret
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
fn guess_upstream_info(
    py: Python,
    path: std::path::PathBuf,
    trust_package: Option<bool>,
) -> PyResult<Vec<PyObject>> {
    let mut result = Vec::new();

    for datum in upstream_ontologist::guess_upstream_info(&path, trust_package) {
        let datum = match datum {
            Ok(datum) => datum,
            Err(e) => {
                log::warn!("Warning: {}", e);
                continue;
            }
        };
        result.push(datum.to_object(py));
    }

    Ok(result)
}

#[pyfunction]
fn description_from_readme_md(
    py: Python,
    contents: &str,
) -> PyResult<(Option<String>, Vec<PyObject>)> {
    let (description, metadata) =
        upstream_ontologist::readme::description_from_readme_md(contents)?;
    let metadata = metadata
        .into_iter()
        .map(|datum| datum.to_object(py))
        .collect();
    Ok((description, metadata))
}

#[pyfunction]
fn get_upstream_info(
    py: Python,
    path: std::path::PathBuf,
    trust_package: Option<bool>,
    net_access: Option<bool>,
    consult_external_directory: Option<bool>,
    check: Option<bool>,
) -> PyResult<&PyDict> {
    let metadata = upstream_ontologist::get_upstream_info(
        path.as_path(),
        trust_package,
        net_access,
        consult_external_directory,
        check,
    )?;
    let ret = PyDict::new(py);
    for datum in metadata.iter() {
        ret.set_item(
            datum.datum.field(),
            datum
                .datum
                .to_object(py)
                .extract::<(String, PyObject)>(py)?
                .1,
        )?;
    }
    Ok(ret)
}

#[pyfunction]
fn check_upstream_metadata(metadata: &mut UpstreamMetadata) -> PyResult<()> {
    upstream_ontologist::check_upstream_metadata(&mut metadata.0, None);
    Ok(())
}

#[pyfunction]
fn extend_upstream_metadata(
    metadata: &mut UpstreamMetadata,
    path: std::path::PathBuf,
    minimum_certainty: Option<String>,
    net_access: Option<bool>,
    consult_external_directory: Option<bool>,
) -> PyResult<()> {
    let minimum_certainty = minimum_certainty.map(|s| s.parse()).transpose().map_err(|e: String| {
        PyValueError::new_err(format!(
            "Invalid minimum_certainty: {}",
            e.to_string()
        ))
    })?;
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
fn guess_upstream_metadata_items(
    py: Python,
    path: std::path::PathBuf,
    trust_package: Option<bool>,
    minimum_certainty: Option<String>,
) -> PyResult<Vec<PyObject>> {
    let metadata = upstream_ontologist::guess_upstream_metadata_items(
        path.as_path(),
        trust_package,
        minimum_certainty.map(|s| s.parse()).transpose().map_err(|e: String| {
            PyValueError::new_err(format!(
                "Invalid minimum_certainty: {}",
                e.to_string()
            ))
        })?,
    );
    Ok(metadata
        .into_iter()
        .map(|datum| datum.map(|o| o.to_object(py)))
        .collect::<Result<Vec<PyObject>, upstream_ontologist::ProviderError>>()?)
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
    items_iter: PyObject
) -> PyResult<Vec<UpstreamDatum>> {
    let mut items = vec![];
    loop {
        let item = match items_iter.call_method0(py, "__next__") {
            Ok(item) => item,
            Err(e) => {
                if e.is_instance_of::<PyStopIteration>(py) {
                    break;
                } else {
                    return Err(e);
                }
            }
        };
        items.push(item.extract::<UpstreamDatum>(py)?);
    }
    Ok(upstream_ontologist::update_from_guesses(
        metadata.0.mut_items(),
        items.into_iter().map(|datum| datum.0),
    ).into_iter().map(UpstreamDatum).collect())
}

#[pymodule]
fn _upstream_ontologist(py: Python, m: &PyModule) -> PyResult<()> {
    pyo3_log::init();
    m.add_wrapped(wrap_pyfunction!(url_from_git_clone_command))?;
    m.add_wrapped(wrap_pyfunction!(url_from_vcs_command))?;
    m.add_wrapped(wrap_pyfunction!(url_from_fossil_clone_command))?;
    m.add_wrapped(wrap_pyfunction!(url_from_svn_co_command))?;
    m.add_wrapped(wrap_pyfunction!(url_from_cvs_co_command))?;
    m.add_wrapped(wrap_pyfunction!(drop_vcs_in_scheme))?;
    m.add_wrapped(wrap_pyfunction!(unsplit_vcs_url))?;
    m.add_wrapped(wrap_pyfunction!(plausible_vcs_url))?;
    m.add_wrapped(wrap_pyfunction!(plausible_vcs_browse_url))?;
    m.add_wrapped(wrap_pyfunction!(check_url_canonical))?;
    m.add_wrapped(wrap_pyfunction!(guess_repo_from_url))?;
    m.add_wrapped(wrap_pyfunction!(probe_gitlab_host))?;
    m.add_wrapped(wrap_pyfunction!(is_gitlab_site))?;
    m.add_wrapped(wrap_pyfunction!(check_repository_url_canonical))?;
    m.add_wrapped(wrap_pyfunction!(probe_upstream_branch_url))?;
    m.add_wrapped(wrap_pyfunction!(guess_from_launchpad))?;
    m.add_wrapped(wrap_pyfunction!(canonical_git_repo_url))?;
    m.add_wrapped(wrap_pyfunction!(browse_url_from_repo_url))?;
    m.add_wrapped(wrap_pyfunction!(find_public_repo_url))?;
    m.add_wrapped(wrap_pyfunction!(find_forge))?;
    m.add_wrapped(wrap_pyfunction!(repo_url_from_merge_request_url))?;
    m.add_wrapped(wrap_pyfunction!(bug_database_from_issue_url))?;
    m.add_wrapped(wrap_pyfunction!(guess_bug_database_url_from_repo_url))?;
    m.add_wrapped(wrap_pyfunction!(bug_database_url_from_bug_submit_url))?;
    m.add_wrapped(wrap_pyfunction!(bug_submit_url_from_bug_database_url))?;
    m.add_wrapped(wrap_pyfunction!(check_bug_database_canonical))?;
    m.add_wrapped(wrap_pyfunction!(check_bug_submit_url_canonical))?;
    m.add_wrapped(wrap_pyfunction!(fixup_rcp_style_git_repo_url))?;
    m.add_wrapped(wrap_pyfunction!(check_upstream_metadata))?;
    m.add_wrapped(wrap_pyfunction!(extend_upstream_metadata))?;
    m.add_wrapped(wrap_pyfunction!(guess_upstream_metadata))?;
    m.add_wrapped(wrap_pyfunction!(fix_upstream_metadata))?;
    m.add_wrapped(wrap_pyfunction!(guess_upstream_metadata_items))?;
    m.add_wrapped(wrap_pyfunction!(update_from_guesses))?;
    let debianm = PyModule::new(py, "debian")?;
    debianm.add_wrapped(wrap_pyfunction!(upstream_package_to_debian_source_name))?;
    debianm.add_wrapped(wrap_pyfunction!(upstream_package_to_debian_binary_name))?;
    debianm.add_wrapped(wrap_pyfunction!(valid_debian_package_name))?;
    debianm.add_wrapped(wrap_pyfunction!(debian_to_upstream_version))?;
    debianm.add_wrapped(wrap_pyfunction!(upstream_name_to_debian_source_name))?;
    m.add("debian", debianm)?;
    m.add_wrapped(wrap_pyfunction!(find_secure_repo_url))?;
    m.add_wrapped(wrap_pyfunction!(sanitize_url))?;
    m.add_wrapped(wrap_pyfunction!(convert_cvs_list_to_str))?;
    m.add_wrapped(wrap_pyfunction!(fixup_broken_git_details))?;
    m.add_wrapped(wrap_pyfunction!(guess_upstream_info))?;
    m.add_wrapped(wrap_pyfunction!(get_upstream_info))?;
    m.add_wrapped(wrap_pyfunction!(description_from_readme_md))?;
    m.add_class::<Forge>()?;
    m.add_class::<GitHub>()?;
    m.add_class::<GitLab>()?;
    m.add_class::<Launchpad>()?;
    m.add_class::<SourceForge>()?;
    m.add_class::<UpstreamMetadata>()?;
    m.add_class::<UpstreamDatum>()?;
    m.add("InvalidUrl", py.get_type::<InvalidUrl>())?;
    m.add("UnverifiableUrl", py.get_type::<UnverifiableUrl>())?;
    m.add("NoSuchForgeProject", py.get_type::<NoSuchForgeProject>())?;
    m.add_wrapped(wrap_pyfunction!(known_bad_guess))?;
    let readmem = PyModule::new(py, "readme")?;
    readmem.add_wrapped(wrap_pyfunction!(readme_skip_paragraph))?;
    m.add_submodule(readmem)?;
    m.add(
        "ParseError",
        py.get_type::<upstream_ontologist::ParseError>(),
    )?;
    m.add(
        "KNOWN_GITLAB_SITES",
        upstream_ontologist::vcs::KNOWN_GITLAB_SITES.to_vec(),
    )?;
    m.add(
        "SECURE_SCHEMES",
        upstream_ontologist::vcs::SECURE_SCHEMES.to_vec(),
    )?;
    Ok(())
}
