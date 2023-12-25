use crate::{ProviderError, UpstreamDatumWithMetadata};
use pyo3::prelude::*;

pub fn guess_from_homepage(
    url: &url::Url,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    Python::with_gil(|py| {
        let m = py.import("upstream_ontologist.homepage")?;
        let f = m.getattr("guess_from_homepage")?;
        let result = f.call1((url.as_str(),))?;
        result.extract()
    })
    .map_err(ProviderError::Python)
}
