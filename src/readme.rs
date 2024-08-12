use crate::{Certainty, Origin, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use lazy_regex::regex;
use pyo3::prelude::*;
use select::document::Document;
use select::node::Node;
use select::predicate::{Name, Predicate};
use std::io::BufRead;
use std::iter::Iterator;

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
    let html = rst_to_html(long_description);

    description_from_readme_html(&html)
}

pub fn description_from_readme_md(
    long_description: &str,
) -> PyResult<(Option<String>, Vec<UpstreamDatumWithMetadata>)> {
    let parser = pulldown_cmark::Parser::new(long_description);

    let mut html_output = String::new();
    pulldown_cmark::html::push_html(&mut html_output, parser);

    description_from_readme_html(&html_output)
}

pub fn guess_from_readme(
    path: &std::path::Path,
    _trust_package: bool,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut urls: Vec<url::Url> = vec![];
    let mut ret = vec![];

    let f = std::fs::File::open(path)?;
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

        if cmdline.starts_with("git clone ")
            || cmdline.starts_with("fossil clone ")
            || cmdline.starts_with("hg clone ")
            || cmdline.starts_with("bzr co ")
            || cmdline.starts_with("bzr branch ")
        {
            while cmdline.ends_with('\\') {
                let next_line = line_iter.next().unwrap()?;
                cmdline = format!("{} {}", cmdline, next_line.trim());
            }

            if let Some(url) = crate::vcs_command::url_from_vcs_command(cmdline.as_bytes()) {
                urls.push(url.parse().unwrap());
            }
        }
        for m in lazy_regex::regex!("[\"'`](git clone.*)[\"`']").captures_iter(line) {
            if let Some(url) = crate::vcs_command::url_from_git_clone_command(
                m.get(1).unwrap().as_str().as_bytes(),
            ) {
                urls.push(url.parse().unwrap());
            }
        }
        if let Some(m) = lazy_regex::regex_find!(r"cvs.*-d\s*:pserver:.*", line) {
            if let Some(url) = crate::vcs_command::url_from_cvs_co_command(m.as_bytes()) {
                urls.push(url.parse().unwrap());
            }
        }
        for m in lazy_regex::regex!("($ )?(svn co .*)").captures_iter(line) {
            if let Some(url) =
                crate::vcs_command::url_from_svn_co_command(m.get(2).unwrap().as_str().as_bytes())
            {
                urls.push(url.parse().unwrap());
            }
        }
        const PROJECT_RE: &str = "([^/]+)/([^/?.()\"#>\\s]*[^-,/?.()\"#>\\s])";
        for m in regex::Regex::new(format!("https://travis-ci.org/{}", PROJECT_RE).as_str())
            .unwrap()
            .captures_iter(line)
        {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(format!(
                    "https://github.com/{}/{}",
                    m.get(1).unwrap().as_str(),
                    m.get(2).unwrap().as_str()
                )),
                certainty: Some(Certainty::Possible),
                origin: Some(path.into()),
            });
        }
        for m in regex::Regex::new(format!("https://coveralls.io/r/{}", PROJECT_RE).as_str())
            .unwrap()
            .captures_iter(line)
        {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(format!(
                    "https://github.com/{}/{}",
                    m.get(1).unwrap().as_str(),
                    m.get(2).unwrap().as_str()
                )),
                certainty: Some(Certainty::Possible),
                origin: Some(path.into()),
            });
        }
        for m in lazy_regex::regex!("https://github.com/([^/]+)/([^/]+)/issues").find_iter(line) {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::BugDatabase(m.as_str().to_string()),
                certainty: Some(Certainty::Possible),
                origin: Some(path.into()),
            });
        }
        for m in regex::Regex::new(format!("https://github.com/{}/(.git)?", PROJECT_RE).as_str())
            .unwrap()
            .find_iter(line)
        {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(m.as_str().trim_end_matches('.').to_string()),
                certainty: Some(Certainty::Possible),
                origin: Some(path.into()),
            });
        }
        for m in regex::Regex::new(format!("https://github.com/{}", PROJECT_RE).as_str())
            .unwrap()
            .captures_iter(line)
        {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(
                    m.get(0).unwrap().as_str().trim_end_matches('.').to_string(),
                ),
                certainty: Some(Certainty::Possible),
                origin: Some(path.into()),
            });
        }
        if let Some(m) = lazy_regex::regex_find!(r"git://([^ ]+)", line) {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(m.trim_end_matches('.').to_string()),
                certainty: Some(Certainty::Possible),
                origin: Some(path.into()),
            });
        }
        for m in lazy_regex::regex_find!("https://([^]/]+)/([^]\\s()\"#]+)", line) {
            let url = m.trim_end_matches('.');
            if crate::vcs::is_gitlab_site(m, None) {
                if let Some(repo_url) = crate::vcs::guess_repo_from_url(&url.parse().unwrap(), None)
                {
                    ret.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Repository(repo_url),
                        certainty: Some(Certainty::Possible),
                        origin: Some(path.into()),
                    });
                } else {
                    log::warn!("Ignoring invalid URL {} in {}", url, path.display());
                }
            }
        }
    }

    let (description, extra_metadata) = match path.extension().and_then(|s| s.to_str()) {
        Some("md") => {
            let contents = std::fs::read_to_string(path)?;
            description_from_readme_md(&contents)
        }
        Some("rst") => {
            let contents = std::fs::read_to_string(path)?;
            description_from_readme_rst(&contents)
        }
        None => {
            let contents = std::fs::read_to_string(path)?;
            Ok(description_from_readme_plain(&contents)?)
        }
        Some("pod") => {
            let contents = std::fs::read_to_string(path)?;
            let metadata = crate::providers::perl::guess_from_pod(
                &contents,
                &Origin::Path(path.to_path_buf()),
            )?;
            Ok((None, metadata))
        }
        _ => Ok((None, vec![])),
    }
    .map_err(ProviderError::Python)?;
    if let Some(description) = description {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Description(description),
            certainty: Some(Certainty::Possible),
            origin: Some(path.into()),
        });
    }
    ret.extend(extra_metadata);

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
            origin: Some(path.into()),
        });
    }
    Ok(ret)
}

pub fn parse_first_header_text(text: &str) -> (Option<&str>, Option<&str>, Option<&str>) {
    if let Some((_, name, version)) = lazy_regex::regex_captures!(r"^([A-Za-z]+) ([0-9.]+)$", text)
    {
        return (Some(name), None, Some(version));
    }
    if let Some((_, name, summary)) = lazy_regex::regex_captures!(r"^([A-Za-z]+): (.+)$", text) {
        return (Some(name), Some(summary), None);
    }
    if let Some((_, name, summary)) = lazy_regex::regex_captures!(r"^([A-Za-z]+) - (.+)$", text) {
        return (Some(name), Some(summary), None);
    }
    if let Some((_, name, summary)) = lazy_regex::regex_captures!(r"^([A-Za-z]+) -- (.+)$", text) {
        return (Some(name), Some(summary), None);
    }
    if let Some((_, name, version)) =
        lazy_regex::regex_captures!(r"^([A-Za-z]+) version ([^ ]+)", text)
    {
        return (Some(name), None, Some(version));
    }
    (None, None, None)
}

#[test]
fn test_parse_first_header_text() {
    assert_eq!(
        parse_first_header_text("libwand 1.0"),
        (Some("libwand"), None, Some("1.0"))
    );
    assert_eq!(
        parse_first_header_text("libwand -- A wand"),
        (Some("libwand"), Some("A wand"), None)
    );
    assert_eq!(
        parse_first_header_text("libwand version 1.0"),
        (Some("libwand"), None, Some("1.0"))
    );
}

pub fn description_from_readme_plain(
    text: &str,
) -> Result<(Option<String>, Vec<UpstreamDatumWithMetadata>), ProviderError> {
    let mut lines: Vec<&str> = text.split_terminator('\n').collect();
    let mut metadata: Vec<UpstreamDatumWithMetadata> = Vec::new();

    if lines.is_empty() {
        return Ok((None, Vec::new()));
    }

    if !lines[0].trim().is_empty()
        && lines.len() > 1
        && (lines[1].is_empty() || !lines[1].chars().next().unwrap().is_alphanumeric())
    {
        let (name, summary, version) = parse_first_header_text(lines[0]);
        if let Some(name) = name {
            metadata.push(UpstreamDatumWithMetadata {
                origin: None,
                datum: UpstreamDatum::Name(name.to_string()),
                certainty: Some(Certainty::Likely),
            });
        }
        if let Some(version) = version {
            metadata.push(UpstreamDatumWithMetadata {
                origin: None,
                datum: UpstreamDatum::Version(version.to_string()),
                certainty: Some(Certainty::Likely),
            });
        }
        if let Some(summary) = summary {
            metadata.push(UpstreamDatumWithMetadata {
                origin: None,
                datum: UpstreamDatum::Summary(summary.to_string()),
                certainty: Some(Certainty::Likely),
            });
        }
        if name.is_some() || version.is_some() || summary.is_some() {
            lines.remove(0);
        }
    }

    while !lines.is_empty() && lines[0].trim().trim_matches('-').is_empty() {
        lines.remove(0);
    }

    let mut paras: Vec<Vec<&str>> = Vec::new();
    let mut current_para: Vec<&str> = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            if !current_para.is_empty() {
                paras.push(current_para.clone());
                current_para.clear();
            }
        } else {
            current_para.push(line);
        }
    }
    if !current_para.is_empty() {
        paras.push(current_para.clone());
    }

    let mut output: Vec<String> = Vec::new();
    for para in paras {
        if para.is_empty() {
            continue;
        }
        let line = para.join("\n");
        let (skip, extra_metadata) = skip_paragraph(&line);
        metadata.extend(extra_metadata);
        if skip {
            continue;
        }
        output.push(format!("{}\n", line));
    }
    let description = if output.len() > 30 {
        None
    } else {
        while !output.is_empty() && output.last().unwrap().trim().is_empty() {
            output.pop();
        }
        Some(output.join("\n"))
    };
    Ok((description, metadata))
}

fn ul_is_field_list(el: Node) -> bool {
    let names = ["Issues", "Home", "Documentation", "License"];
    for li in el.find(Name("li")) {
        let text = li.text();
        if let Some((_, name)) = lazy_regex::regex_captures!(r"([A-Za-z]+)\s*:.*", text.trim()) {
            if !names.contains(&name) {
                return false;
            }
        } else {
            return false;
        }
    }
    true
}

#[test]
fn test_ul_is_field_list() {
    let el = Document::from(
        r#"<html><body><ul>
            <li>Issues: <a href="https://github.com/serde-rs/serde/issues">blah</a></li>
            <li>Home: <a href="https://serde.rs/">blah</a></li>
            </ul></body></html>"#,
    );

    let ul = el.find(Name("ul")).next().unwrap();

    assert!(ul_is_field_list(ul));

    let el = Document::from(
        r#"<html><body><ul>
            <li>Some other thing</li>
            </ul></body></html>"#,
    );

    let ul = el.find(Name("ul")).next().unwrap();

    assert!(!ul_is_field_list(ul));
}

pub fn description_from_readme_html(
    long_description: &str,
) -> PyResult<(Option<String>, Vec<UpstreamDatumWithMetadata>)> {
    Python::with_gil(|py| {
        let readme_mod = Python::import_bound(py, "upstream_ontologist.readme").unwrap();
        let (description, extra_md): (Option<String>, Vec<UpstreamDatumWithMetadata>) = readme_mod
            .call_method1("description_from_readme_html", (long_description,))?
            .extract()?;

        Ok((description, extra_md))
    })
}

fn rst_to_html(rst_text: &str) -> String {
    use rst_parser::parse;
    use rst_renderer::render_html;
    let document = parse(rst_text).unwrap();
    let mut output = Vec::new();
    render_html(&document, &mut std::io::Cursor::new(&mut output), true).unwrap();
    String::from_utf8(output).unwrap()
}
