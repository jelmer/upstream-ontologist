use crate::{Certainty, Origin, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use lazy_regex::regex;
use pyo3::prelude::*;
use regex::Regex;
use select::document::Document;
use select::node::Node;
use select::predicate::{And, Class, Name, Predicate, Text};
use std::io::BufRead;
use std::iter::Iterator;
use url::Url;

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
    // Work around https://github.com/flying-sheep/rust-rst/issues/55

    let mut fields: Vec<(&str, String)> = Vec::new();
    let mut in_field = false;

    let long_description = long_description
        .lines()
        .filter(|line| {
            // Filter out field lists. Syntax is:
            // :field: value
            // with possible continuation lines that are indented.
            // field can contain any character except a colon followed by a space unless
            // it is escaped with a backslash.
            if line.starts_with([' ', '\t'].as_ref()) && in_field {
                if in_field {
                    fields.last_mut().unwrap().1.push_str(line.trim());
                    return false;
                }
                return true;
            } else {
                in_field = false;
            }
            if let Some((_, field, value)) = lazy_regex::regex_captures!(r"^:([^:]+): (.*)", line) {
                fields.push((field, value.to_string()));
                in_field = true;
                false
            } else {
                line != &"----"
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    let html = rst_to_html(&long_description);

    let (description, mut md) = description_from_readme_html(&html)?;

    for (field, value) in fields {
        md.extend(parse_field(field, &NodeOrText::Text(&value)));
    }

    Ok((description, md))
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

fn skip_paragraph_block(para: &Node) -> (bool, Vec<UpstreamDatumWithMetadata>) {
    let (skip, mut extra_metadata) = skip_paragraph(&render(para));

    if skip {
        return (true, extra_metadata);
    }

    for child in para.children() {
        if let Some(text_node) = child.as_text() {
            if text_node.trim().is_empty() {
                continue;
            }
        }

        if child.name() == Some("a") {
            let mut name: Option<String> = None;
            if let Some(first_child) = para.first_child() {
                if let Some(text) = first_child.as_text() {
                    name = Some(text.to_string());
                } else if first_child.name() == Some("img") {
                    name = first_child.attr("alt").map(|s| s.to_string());
                }
            }

            if let Some(name) = name {
                match name.as_str() {
                    "CRAN" | "CRAN_Status_Badge" | "CRAN_Logs_Badge" => {
                        extra_metadata.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Archive("CRAN".to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: None,
                        });
                    }
                    "Gitter" => {
                        if let Some(href) = child.attr("href") {
                            let parsed_url = Url::parse(href).unwrap();
                            extra_metadata.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::Repository(format!(
                                    "https://github.com/{}",
                                    parsed_url.path().trim_start_matches('/')
                                )),
                                certainty: Some(Certainty::Confident),
                                origin: None,
                            });
                        }
                    }
                    "Build Status" => {
                        if let Some(href) = child.attr("href") {
                            let parsed_url = Url::parse(href).unwrap();
                            if parsed_url.host_str() == Some("travis-ci.org") {
                                extra_metadata.push(UpstreamDatumWithMetadata {
                                    datum: UpstreamDatum::Repository(format!(
                                        "https://github.com/{}",
                                        parsed_url.path().trim_start_matches('/')
                                    )),
                                    certainty: Some(Certainty::Confident),
                                    origin: None,
                                });
                            }
                        }
                    }
                    "Documentation" => {
                        if let Some(href) = child.attr("href") {
                            extra_metadata.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::Documentation(href.to_string()),
                                certainty: Some(Certainty::Confident),
                                origin: None,
                            });
                        }
                    }
                    "API Docs" => {
                        if let Some(href) = child.attr("href") {
                            extra_metadata.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::APIDocumentation(href.to_string()),
                                certainty: Some(Certainty::Confident),
                                origin: None,
                            });
                        }
                    }
                    "Downloads" => {
                        if let Some(href) = child.attr("href") {
                            extra_metadata.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::Download(href.to_string()),
                                certainty: Some(Certainty::Confident),
                                origin: None,
                            });
                        }
                    }
                    "crates.io" => {
                        if let Some(href) = child.attr("href") {
                            if href.starts_with("https://crates.io/crates/") {
                                extra_metadata.push(UpstreamDatumWithMetadata {
                                    datum: UpstreamDatum::CargoCrate(
                                        href.rsplit('/').next().unwrap().to_string(),
                                    ),
                                    certainty: Some(Certainty::Confident),
                                    origin: None,
                                });
                            }
                        }
                    }
                    name => {
                        let re = Regex::new(r"(.*) License").unwrap();
                        if let Some(caps) = re.captures(name) {
                            extra_metadata.push(UpstreamDatumWithMetadata {
                                datum: UpstreamDatum::License(caps[1].to_string()),
                                certainty: Some(Certainty::Likely),
                                origin: None,
                            });
                        } else {
                            log::debug!("Unhandled field {:?} in README", name);
                        }
                    }
                }
            }
        }
    }

    if render(para).is_empty() {
        return (true, extra_metadata);
    }

    (false, vec![])
}

fn render(el: &Node) -> String {
    el.find(Text).map(|t| t.text()).collect::<Vec<_>>().join("")
}

fn parse_first_header(el: &Node) -> Vec<UpstreamDatumWithMetadata> {
    let mut metadata = Vec::new();
    let binding = render(el);
    let (name, summary, version) = parse_first_header_text(&binding);

    if let Some(mut name) = name {
        if name.to_lowercase().contains("installation") {
            metadata.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(name.to_string()),
                certainty: Some(Certainty::Possible),
                origin: None,
            });
        } else {
            metadata.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(name.to_string()),
                certainty: Some(Certainty::Likely),
                origin: None,
            });
        }

        if let Some(suffix) = name.strip_prefix("About ") {
            name = suffix;
        }

        metadata.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(name.to_string()),
            certainty: Some(Certainty::Likely),
            origin: None,
        });
    }

    if let Some(summary) = summary {
        metadata.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Summary(summary.to_string()),
            certainty: Some(Certainty::Likely),
            origin: None,
        });
    }

    if let Some(version) = version {
        metadata.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Version(version.to_string()),
            certainty: Some(Certainty::Likely),
            origin: None,
        });
    }

    metadata
}

fn is_semi_header(el: &Node) -> bool {
    if el.name() != Some("p") {
        return false;
    }

    let text = render(el);
    if text == "INSTALLATION" {
        return true;
    }

    if text.contains('\n') {
        return false;
    }

    let re = Regex::new(r"([a-z-A-Z0-9]+) - ([^\.]+)").unwrap();
    re.is_match(&text)
}

fn extract_paragraphs<'a>(
    children: impl Iterator<Item = Node<'a>>,
    paragraphs: &mut Vec<String>,
    metadata: &mut Vec<UpstreamDatumWithMetadata>,
) {
    for child in children {
        match child.name() {
            Some("div") => {
                extract_paragraphs(child.children(), paragraphs, metadata);
                if !paragraphs.is_empty() && child.is(Class("section")) {
                    break;
                }
            }
            Some("section") => {
                extract_paragraphs(child.children(), paragraphs, metadata);
                if !paragraphs.is_empty() {
                    break;
                }
            }
            Some("p") => {
                if is_semi_header(&child) {
                    if paragraphs.is_empty() {
                        metadata.extend(parse_first_header(&child));
                        continue;
                    } else {
                        break;
                    }
                }
                let (skip, extra_metadata) = skip_paragraph_block(&child);
                metadata.extend(extra_metadata);

                if skip {
                    if paragraphs.is_empty() {
                        continue;
                    } else {
                        break;
                    }
                }

                let text = render(&child);
                if !text.trim().is_empty() {
                    paragraphs.push(text + "\n");
                }
            }
            Some("pre") => paragraphs.push(render(&child)),
            Some("ul") if !paragraphs.is_empty() => {
                if ul_is_field_list(child) {
                    metadata.extend(parse_ul_field_list(&child));
                } else {
                    paragraphs.push(
                        child
                            .find(Name("li"))
                            .map(|li| format!("* {}\n", render(&li)))
                            .collect::<Vec<_>>()
                            .join(""),
                    );
                }
            }
            Some(h) if h.starts_with("h") => {
                if paragraphs.is_empty() {
                    if !["About", "Introduction", "Overview", "Documentation"]
                        .contains(&render(&child).trim())
                    {
                        metadata.extend(parse_first_header(&child));
                    }
                } else {
                    break;
                }
            }
            None => {}
            _ => {
                log::debug!("Unhandled element in README: {:?}", child.name());
            }
        }
    }
}

fn parse_field(name: &str, body: &NodeOrText) -> Vec<UpstreamDatumWithMetadata> {
    let mut metadata = Vec::new();

    let get_link = || -> Option<String> {
        match body {
            NodeOrText::Node(body) => {
                if let Some(a) = body.find(Name("a")).next() {
                    return Some(a.attr("href").unwrap().to_string());
                } else if body.is(Name("a")) {
                    return Some(body.attr("href").unwrap().to_string());
                } else if let Some(text) = body.as_text().filter(|u| Url::parse(u).is_ok()) {
                    return Some(text.to_string());
                } else {
                    return None;
                }
            }
            NodeOrText::Text(text) => {
                if let Ok(url) = Url::parse(text) {
                    return Some(url.to_string());
                }
                None
            }
        }
    };

    match name {
        "Homepage" | "Home" => {
            if let Some(link) = get_link() {
                metadata.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Homepage(link),
                    certainty: Some(Certainty::Confident),
                    origin: None,
                });
            }
        }

        "Issues" => {
            if let Some(link) = get_link() {
                metadata.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::BugDatabase(link),
                    certainty: Some(Certainty::Confident),
                    origin: None,
                });
            }
        }

        "Documentation" => {
            if let Some(link) = get_link() {
                metadata.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Documentation(link),
                    certainty: Some(Certainty::Confident),
                    origin: None,
                });
            }
        }

        "License" => {
            metadata.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::License(match body {
                    NodeOrText::Node(body) => render(body),
                    NodeOrText::Text(text) => text.to_string(),
                }),
                certainty: Some(Certainty::Confident),
                origin: None,
            });
        }

        _ => {
            log::debug!("Unhandled field {:?} in README", name);
        }
    }

    metadata
}

enum NodeOrText<'a> {
    Node(Node<'a>),
    Text(&'a str),
}

impl<'a> From<Node<'a>> for NodeOrText<'a> {
    fn from(node: Node<'a>) -> Self {
        if let Some(text) = node.as_text() {
            NodeOrText::Text(text)
        } else {
            NodeOrText::Node(node)
        }
    }
}

impl<'a> From<&'a str> for NodeOrText<'a> {
    fn from(text: &'a str) -> Self {
        NodeOrText::Text(text)
    }
}

/// Extracts a list of fields from a `ul` element.
///
/// # Arguments
/// * `el` - The `ul` element to extract fields from.
///
/// # Returns
/// A list of fields extracted from the `ul` element.
fn iter_ul_field_list<'a>(el: &'a Node<'a>) -> Vec<(&'a str, NodeOrText<'a>)> {
    el.find(Name("li"))
        .filter_map(|li| {
            let children: Vec<_> = li.children().collect();
            if children.len() == 2 && children[0].is(Text) {
                let name = children[0].as_text().unwrap().trim().trim_end_matches(':');
                return Some((name, children[1].into()));
            } else if children.len() == 1 {
                let (name, value) = children[0].as_text().unwrap().split_once(':')?;
                return Some((
                    name.trim(),
                    NodeOrText::Text(value.trim().trim_start_matches(':')),
                ));
            }
            None
        })
        .collect()
}

/// Parses a list of fields from a `ul` element.
///
/// # Arguments
/// * `el` - The `ul` element to parse.
///
/// # Returns
/// A list of metadata extracted from the `ul` element.
fn parse_ul_field_list(el: &Node) -> Vec<UpstreamDatumWithMetadata> {
    let mut metadata = Vec::new();

    for (name, el_ref) in iter_ul_field_list(el) {
        metadata.extend(parse_field(name, &el_ref));
    }

    metadata
}

fn description_from_basic_soup(
    soup: &Document,
) -> (Option<String>, Vec<UpstreamDatumWithMetadata>) {
    let mut metadata = Vec::new();

    let body = soup
        .find(Name("body"))
        .next()
        .expect("No body element found in HTML document");

    let mut child_iter = body.children().peekable();

    // Drop any headers
    while let Some(el) = child_iter.peek() {
        if el.name().map(|h| h.starts_with("h")).unwrap_or(false) {
            metadata.extend(parse_first_header(el));
            child_iter.next();
        } else if el.is(Text) {
            child_iter.next();
            continue;
        } else {
            break;
        }
    }

    if let Some(table) = soup.find(And(Name("table"), Class("field-list"))).next() {
        metadata.extend(parse_ul_field_list(&table));
    }

    let mut paragraphs: Vec<String> = Vec::new();

    extract_paragraphs(child_iter, &mut paragraphs, &mut metadata);

    if paragraphs.is_empty() {
        log::debug!("Empty description; no paragraphs.");
        return (None, metadata);
    }

    if paragraphs.len() < 6 {
        return (Some(paragraphs.join("\n")), metadata);
    }

    log::debug!(
        "Not returning description, number of paragraphs too high: {}",
        paragraphs.len()
    );
    (None, metadata)
}

pub fn description_from_readme_html(
    html_text: &str,
) -> PyResult<(Option<String>, Vec<UpstreamDatumWithMetadata>)> {
    let soup = Document::from(html_text);
    Ok(description_from_basic_soup(&soup))
}

fn rst_to_html(rst_text: &str) -> String {
    use rst_parser::parse;
    use rst_renderer::render_html;
    let document = parse(rst_text).unwrap();
    let mut output = Vec::new();
    render_html(&document, &mut std::io::Cursor::new(&mut output), true).unwrap();
    String::from_utf8(output).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rst_to_html() {
        let rst = r#".. _`rst`:

RST
===

This is a test of RST to HTML conversion."#;
        let html = rst_to_html(rst);
        assert_eq!(
            html,
            "<!doctype html><html>\n\n<section id=\"rst\">\n<h1>RST</h1>\n<p>This is a test of RST to HTML conversion.</p>\n</section>\n</html>\n"
        );
    }

    #[test]
    fn test_parse_first_header_text() {
        assert_eq!(
            super::parse_first_header_text("libwand 1.0"),
            (Some("libwand"), None, Some("1.0"))
        );
        assert_eq!(
            super::parse_first_header_text("libwand -- A wand"),
            (Some("libwand"), Some("A wand"), None)
        );
        assert_eq!(
            super::parse_first_header_text("libwand version 1.0"),
            (Some("libwand"), None, Some("1.0"))
        );
    }

    #[test]
    fn test_parse_field() {
        assert_eq!(
            super::parse_field(
                "Homepage",
                &root(&Document::from(
                    r#"<a href="https://example.com">example</a>"#
                ))
                .into()
            ),
            vec![super::UpstreamDatumWithMetadata {
                datum: super::UpstreamDatum::Homepage("https://example.com".to_string()),
                certainty: Some(super::Certainty::Confident),
                origin: None,
            }]
        );

        assert_eq!(
            super::parse_field(
                "Issues",
                &root(&Document::from(
                    r#"<a href="https://example.com">example</a>"#
                ))
                .into(),
            ),
            vec![super::UpstreamDatumWithMetadata {
                datum: super::UpstreamDatum::BugDatabase("https://example.com".to_string()),
                certainty: Some(super::Certainty::Confident),
                origin: None,
            }]
        );

        assert_eq!(
            super::parse_field(
                "Documentation",
                &root(&Document::from(
                    r#"<a href="https://example.com">example</a>"#
                ))
                .into()
            ),
            vec![super::UpstreamDatumWithMetadata {
                datum: super::UpstreamDatum::Documentation("https://example.com".to_string()),
                certainty: Some(super::Certainty::Confident),
                origin: None,
            }]
        );

        assert_eq!(
            super::parse_field("License", &"MIT".into()),
            vec![super::UpstreamDatumWithMetadata {
                datum: super::UpstreamDatum::License("MIT".to_string()),
                certainty: Some(super::Certainty::Confident),
                origin: None,
            }]
        );
    }

    fn root(doc: &Document) -> Node {
        let root = doc.find(Root).next().unwrap();
        assert_eq!(root.name(), Some("html"));
        root.find(Name("body"))
            .next()
            .unwrap()
            .first_child()
            .unwrap()
    }

    #[test]
    fn test_is_semi_header() {
        let fragment = Document::from("<p>INSTALLATION</p>");
        assert!(root(&fragment).name() == Some("p"));
        assert!(super::is_semi_header(&root(&fragment)));

        let fragment = Document::from("<p>Some other thing</p>");

        assert!(!super::is_semi_header(&root(&fragment)));
    }

    #[test]
    fn test_iter_ul_field_list() {
        let fragment = Document::from(
            r#"<ul>
            <li>Issues: <a href="https://example.com/issues">example</a></li>
            <li>Home: <a href="https://example.com">example</a></li>
            </ul>"#,
        );

        assert_eq!(Some("ul"), root(&fragment).name());

        assert_eq!(
            super::iter_ul_field_list(&root(&fragment))
                .iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            vec![&"Issues", &"Home"]
        );
    }

    #[test]
    fn test_parse_ul_field_list() {
        let fragment = Document::from(
            r#"<ul>
            <li>Issues: <a href="https://example.com/issues">example</a></li>
            <li>Home: <a href="https://example.com">example</a></li>
            <li>Documentation: <a href="https://example.com/docs">example</a></li>
            <li>License: MIT</li>
            </ul>"#,
        );

        assert_eq!(
            super::parse_ul_field_list(&root(&fragment)),
            vec![
                super::UpstreamDatumWithMetadata {
                    datum: super::UpstreamDatum::BugDatabase(
                        "https://example.com/issues".to_string()
                    ),
                    certainty: Some(super::Certainty::Confident),
                    origin: None,
                },
                super::UpstreamDatumWithMetadata {
                    datum: super::UpstreamDatum::Homepage("https://example.com".to_string()),
                    certainty: Some(super::Certainty::Confident),
                    origin: None,
                },
                super::UpstreamDatumWithMetadata {
                    datum: super::UpstreamDatum::Documentation(
                        "https://example.com/docs".to_string()
                    ),
                    certainty: Some(super::Certainty::Confident),
                    origin: None,
                },
                super::UpstreamDatumWithMetadata {
                    datum: super::UpstreamDatum::License("MIT".to_string()),
                    certainty: Some(super::Certainty::Confident),
                    origin: None,
                }
            ]
        );
    }

    #[test]
    fn test_render() {
        let fragment = Document::from("<p>Some text</p>");
        assert_eq!(super::render(&root(&fragment)), "Some text");

        let fragment = Document::from("<p>Some <b>bold</b> text</p>");
        assert_eq!(super::render(&root(&fragment)), "Some bold text");
    }

    #[test]
    fn test_extract_paragraphs() {
        let fragment = Document::from(
            r#"<div>
            <p>Some text</p>
            <p>Some more text</p>
            </div>"#,
        );

        let mut paragraphs = Vec::new();
        super::extract_paragraphs(root(&fragment).children(), &mut paragraphs, &mut vec![]);

        assert_eq!(paragraphs, vec!["Some text\n", "Some more text\n"]);
    }

    #[test]
    fn test_swh() {
        let document = Document::from(include_str!("testdata/swh.html"));
        let (description, metadata) = super::description_from_basic_soup(&document);
        assert_eq!(
            description,
            Some(
                r#"The Software Heritage Git Loader is a tool and a library to walk a local
Git repository and inject into the SWH dataset all contained files that
weren't known before.

The main entry points are:

* 
:class:swh.loader.git.loader.GitLoader for the main loader which can ingest either
local or remote git repository's contents. This is the main implementation deployed in
production.

* 
:class:swh.loader.git.from_disk.GitLoaderFromDisk which ingests only local git clone
repository.

* 
:class:swh.loader.git.loader.GitLoaderFromArchive which ingests a git repository
wrapped in an archive.

"#
                .to_string()
            )
        );
        assert_eq!(metadata, vec![]);
    }
}
