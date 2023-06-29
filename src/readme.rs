use crate::{Certainty, UpstreamDatum, UpstreamDatumWithMetadata};
use lazy_regex::regex;
use pyo3::prelude::*;
use pyo3::types::PyDict;

pub fn skip_paragraph(para: &str) -> (bool, Vec<UpstreamDatumWithMetadata>) {
    let mut ret = Vec::<UpstreamDatumWithMetadata>::new();
    let re = regex!(r"(?ms)^See .* for more (details|information)\.");
    if re.is_match(para) {
        return (true, ret);
    }

    let re = regex!(r"(?ms)^See .* for instructions");
    if re.is_match(para) {
        return (true, ret);
    }

    let re = regex!(r"(?ms)^Please refer .*\.");
    if re.is_match(para) {
        return (true, ret);
    }

    if let Some(m) = regex!(r"(?ms)^It is licensed under (.*)").captures(para) {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::License(m.get(1).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Possible),
            origin: None,
        });
        return (true, ret);
    }

    if let Some(m) = regex!(r"(?ms)^License: (.*)").captures(para) {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::License(m.get(1).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Likely),
            origin: None,
        });
        return (true, ret);
    }

    if let Some(m) =
        regex!(r"(?ms)^(Home page|homepage_url|Main website|Website|Homepage): (.*)").captures(para)
    {
        let mut url = m.get(2).unwrap().as_str().to_string();
        if url.starts_with('<') && url.ends_with('>') {
            url = url[1..url.len() - 1].to_string();
        }
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Homepage(url),
            certainty: Some(Certainty::Likely),
            origin: None,
        });
        return (true, ret);
    }

    if regex!(r"(?ms)^More documentation .* at http.*").is_match(para) {
        return (true, ret);
    }

    if let Some(m) =
        regex!(r"(?ms)^Documentation (can be found|is hosted|is available) (at|on) ([^ ]+)")
            .captures(para)
    {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Documentation(m.get(3).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Likely),
            origin: None,
        });
        return (true, ret);
    }

    if let Some(m) =
        regex!(r"(?ms)^Documentation for (.*)\s+(can\s+be\s+found|is\s+hosted)\s+(at|on)\s+([^ ]+)")
            .captures(para)
    {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(m.get(1).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Possible),
            origin: None,
        });
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Documentation(m.get(4).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Likely),
            origin: None,
        });
        return (true, ret);
    }

    if regex!(r"(?ms)^Documentation[, ].*found.*(at|on).*\.").is_match(para) {
        return (true, ret);
    }

    if regex!(r"(?ms)^See (http.*|gopkg.in.*|github.com.*)").is_match(para) {
        return (true, ret);
    }

    if regex!(r"(?ms)^Available on (.*)").is_match(para) {
        return (true, ret);
    }

    if let Some(m) = regex!(r"(?ms)^This software is freely distributable under the (.*) license.*")
        .captures(para)
    {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::License(m.get(1).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Likely),
            origin: None,
        });
        return (true, ret);
    }

    if regex!(r"(?ms)^This .* is hosted at .*").is_match(para) {
        return (true, ret);
    }

    if regex!(r"(?ms)^This code has been developed by .*").is_match(para) {
        return (true, ret);
    }

    if para.starts_with("Download and install using:") {
        return (true, ret);
    }

    if regex!(r"(?ms)^Bugs should be reported by .*").is_match(para) {
        return (true, ret);
    }

    if let Some(m) = regex!(r"(?ms)^The bug tracker can be found at (http[^ ]+[^.])").captures(para)
    {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::BugDatabase(m.get(1).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Likely),
            origin: None,
        });
        return (true, ret);
    }

    if let Some(m) = regex!(r"(?ms)^Copyright (\(c\) |)(.*)").captures(para) {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Copyright(m.get(2).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Possible),
            origin: None,
        });
        return (true, ret);
    }

    if regex!(r"(?ms)^You install .*").is_match(para) {
        return (true, ret);
    }

    if regex!(r"(?ms)^This .* is free software; .*").is_match(para) {
        return (true, ret);
    }

    if let Some(m) = regex!(r"(?ms)^Please report any bugs(.*) to <(.*)>").captures(para) {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::BugDatabase(m.get(2).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Possible),
            origin: None,
        });
        return (true, ret);
    }

    if regex!(r"(?ms)^Share and Enjoy").is_match(para) {
        return (true, ret);
    }

    let lines = para.lines().collect::<Vec<&str>>();
    if !lines.is_empty() && ["perl Makefile.PL", "make", "./configure"].contains(&lines[0].trim()) {
        return (true, ret);
    }

    if regex!(r"(?ms)^For further information, .*").is_match(para) {
        return (true, ret);
    }

    if regex!(r"(?ms)^Further information .*").is_match(para) {
        return (true, ret);
    }

    if let Some(m) = regex!(r"(?ms)^A detailed ChangeLog can be found.*:\s+(http.*)").captures(para)
    {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Changelog(m.get(1).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Possible),
            origin: None,
        });
        return (true, ret);
    }

    (false, ret)
}

pub fn description_from_readme_rst(
    long_description: &str,
) -> PyResult<(Option<String>, Vec<UpstreamDatumWithMetadata>)> {
    Python::with_gil(|py| {
        let readme_mod = Python::import(py, "upstream_ontologist.readme").unwrap();
        let (description, extra_md): (Option<String>, Vec<PyObject>) = readme_mod
            .call_method1("description_from_readme_rst", (long_description,))?
            .extract()?;

        let extra_md = extra_md
            .into_iter()
            .map(|m| crate::py_to_upstream_datum_with_metadata(py, m))
            .collect::<PyResult<Vec<_>>>()?;
        Ok((description, extra_md))
    })
}

pub fn description_from_readme_md(
    long_description: &str,
) -> PyResult<(Option<String>, Vec<UpstreamDatumWithMetadata>)> {
    Python::with_gil(|py| {
        let readme_mod = Python::import(py, "upstream_ontologist.readme").unwrap();
        let (description, extra_md): (Option<String>, Vec<PyObject>) = readme_mod
            .call_method1("description_from_readme_md", (long_description,))?
            .extract()?;
        let extra_md = extra_md
            .into_iter()
            .map(|m| crate::py_to_upstream_datum_with_metadata(py, m))
            .collect::<PyResult<Vec<_>>>()?;
        Ok((description, extra_md))
    })
}
