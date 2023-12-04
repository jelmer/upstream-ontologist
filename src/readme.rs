use crate::{Certainty, UpstreamDatum, UpstreamDatumWithMetadata, ProviderError};
use lazy_regex::regex;
use pyo3::prelude::*;
use std::io::BufRead;

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
        let (description, extra_md): (Option<String>, Vec<UpstreamDatumWithMetadata>) = readme_mod
            .call_method1("description_from_readme_rst", (long_description,))?
            .extract()?;

        Ok((description, extra_md))
    })
}

pub fn description_from_readme_html(
    long_description: &str,
) -> PyResult<(Option<String>, Vec<UpstreamDatumWithMetadata>)> {
    Python::with_gil(|py| {
        let readme_mod = Python::import(py, "upstream_ontologist.readme").unwrap();
        let (description, extra_md): (Option<String>, Vec<UpstreamDatumWithMetadata>) = readme_mod
            .call_method1("description_from_readme_html", (long_description,))?
            .extract()?;

        Ok((description, extra_md))
    })
}

pub fn description_from_readme_md(
    long_description: &str,
) -> PyResult<(Option<String>, Vec<UpstreamDatumWithMetadata>)> {
    let parser = pulldown_cmark::Parser::new(long_description);

    let mut html_output = String::new();
    pulldown_cmark::html::push_html(&mut html_output, parser);

    description_from_readme_html(&html_output)
}

pub fn description_from_readme_plain(
    long_description: &str,
) -> PyResult<(Option<String>, Vec<UpstreamDatumWithMetadata>)> {
    Python::with_gil(|py| {
        let readme_mod = Python::import(py, "upstream_ontologist.readme").unwrap();
        let (description, extra_md): (Option<String>, Vec<UpstreamDatumWithMetadata>) = readme_mod
            .call_method1("description_from_readme_plain", (long_description,))?
            .extract()?;
        Ok((description, extra_md))
    })
}

pub fn guess_from_readme(path: &std::path::Path, _trust_package: bool) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut urls: Vec<url::Url> = vec![];
    let mut ret = vec![];

    let f =std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(f);

    let mut line_iter = reader.lines();

    loop {
        let line = if let Some(line) = line_iter.next() {
            line?
        } else {
            break;
        };

        let line = line.trim();

        let mut cmdline = line.strip_prefix('$').unwrap_or(line).trim().to_string();

        if cmdline.starts_with("git clone ") || cmdline.starts_with("fossil clone ") || cmdline.starts_with("hg clone ") || cmdline.starts_with("bzr co ") || cmdline.starts_with("bzr branch ") {
            while cmdline.ends_with('\\') {
                let next_line = line_iter.next().unwrap()?;
                cmdline = format!("{} {}", cmdline, next_line.trim());
            }

            if let Some(url) = crate::vcs_command::url_from_vcs_command(cmdline.as_bytes()) {
                urls.push(url.parse().unwrap());
            }
        }
        for m in lazy_regex::regex!("[\"'`](git clone.*)[\"`']").captures_iter(line) {
            if let Some(url) = crate::vcs_command::url_from_git_clone_command(m.get(1).unwrap().as_str().as_bytes()) {
                urls.push(url.parse().unwrap());
            }
        }
        if let Some(m) = lazy_regex::regex_find!(r"cvs.*-d\s*:pserver:.*", line) {
            if let Some(url) = crate::vcs_command::url_from_cvs_co_command(m.as_bytes()) {
                urls.push(url.parse().unwrap());
            }
        }
        for m in lazy_regex::regex!("($ )?(svn co .*)").captures_iter(line) {
            if let Some(url) = crate::vcs_command::url_from_svn_co_command(m.get(2).unwrap().as_str().as_bytes()) {
                urls.push(url.parse().unwrap());
            }
        }
        const PROJECT_RE: &str = "([^/]+)/([^/?.()\"#>\\s]*[^-,/?.()\"#>\\s])";
        for m in regex::Regex::new(format!("https://travis-ci.org/{}", PROJECT_RE).as_str()).unwrap().captures_iter(line) {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(
                format!("https://github.com/{}/{}",
                    m.get(1).unwrap().as_str(), m.get(2).unwrap().as_str())),
                certainty: Some(Certainty::Possible),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
        for m in regex::Regex::new(format!("https://coveralls.io/r/{}", PROJECT_RE).as_str()).unwrap().captures_iter(line) {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(
                format!("https://github.com/{}/{}",
                    m.get(1).unwrap().as_str(), m.get(2).unwrap().as_str())),
                certainty: Some(Certainty::Possible),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
        for m in lazy_regex::regex!("https://github.com/([^/]+)/([^/]+)/issues").find_iter(line) {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::BugDatabase(m.as_str().to_string()),
                certainty: Some(Certainty::Possible),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
        for m in regex::Regex::new(format!("https://github.com/{}/(.git)?", PROJECT_RE).as_str()).unwrap().find_iter(line) {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(m.as_str().trim_end_matches('.').to_string()),
                certainty: Some(Certainty::Possible),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
        for m in regex::Regex::new(format!("https://github.com/{}", PROJECT_RE).as_str()).unwrap().captures_iter(line) {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(m.get(0).unwrap().as_str().trim_end_matches('.').to_string()),
                certainty: Some(Certainty::Possible),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
        if let Some(m) = lazy_regex::regex_find!(r"git://([^ ]+)", line) {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(m.trim_end_matches('.').to_string()),
                certainty: Some(Certainty::Possible),
                origin: Some(path.to_string_lossy().to_string()),
            });
        }
        for m in lazy_regex::regex_find!("https://([^]/]+)/([^]\\s()\"#]+)", line) {
            let url = m.trim_end_matches('.');
            if crate::vcs::is_gitlab_site(m, None) {
                if let Some(repo_url) = crate::vcs::guess_repo_from_url(&url.parse().unwrap(), None) {
                    ret.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Repository(repo_url),
                        certainty: Some(Certainty::Possible),
                        origin: Some(path.to_string_lossy().to_string()),
                    });
                } else {
                    log::warn!("Ignoring invalid URL {} in {}", url, path.display());
                }
            }
        }
    }

    let (description, extra_metadata) = match path.extension().and_then(|s| s.to_str()) {
        Some("md")  => {
            let contents = std::fs::read_to_string(path)?;
            description_from_readme_md(&contents)
        },
        Some("rst") => {
            let contents = std::fs::read_to_string(path)?;
            description_from_readme_rst(&contents)
        },
        None => {
            let contents = std::fs::read_to_string(path)?;
            description_from_readme_plain(&contents)
        },
        Some("pod") => {
            let contents = std::fs::read_to_string(path)?;
            let metadata = crate::providers::perl::guess_from_pod(&contents)?;
            Ok((None, metadata))
        }
        _ => {
            Ok((None, vec![]))
        },
    }.map_err(ProviderError::Python)?;
    if let Some(description) = description {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Description(description),
            certainty: Some(Certainty::Possible),
            origin: Some(path.to_string_lossy().to_string()),
        });
    }
    ret.extend(extra_metadata.into_iter());

    let prefer_public = |url: &url::Url| -> i32 {
        if url.scheme().contains("ssh") {
            1
        } else {
            0
        }
    };

    urls.sort_by_key(prefer_public);

    if !urls.is_empty() {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository(urls.remove(0).to_string()),
            certainty: Some(Certainty::Possible),
            origin: Some(path.to_string_lossy().to_string()),
        });
    }
    Ok(ret)
}
