use pyo3::prelude::*;

mod location {
    pub fn cvs_to_url(cvsroot: &str) -> String {
        Python::with_gil(|py| {
            let breezy_location = py.import("breezy.location").unwrap();

            breezy_location
                .call1("cvs_to_url", (cvsroot,))
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }
}
