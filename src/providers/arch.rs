use crate::{vcs, Certainty, UpstreamDatum, USER_AGENT};
use log::{debug, error};
use std::collections::HashMap;
use std::io::BufRead;

pub fn parse_pkgbuild_variables(file: &str) -> HashMap<String, Vec<String>> {
    let reader = std::io::Cursor::new(file);

    let mut variables = HashMap::new();
    let mut keep: Option<(String, String)> = None;
    let mut existing: Option<String> = None;

    for line in reader.lines() {
        let line = line.expect("Failed to read line");
        if let Some(existing_line) = existing.take() {
            let line = [&existing_line[..existing_line.len() - 2], &line].concat();
            existing = Some(line);
            continue;
        }

        if line.ends_with("\\\n") {
            existing = Some(line[..line.len() - 2].to_owned());
            continue;
        }

        if line.starts_with('\t') || line.starts_with(' ') || line.starts_with('#') {
            continue;
        }

        if let Some((key, mut value)) = keep.take() {
            value.push_str(&line);
            if line.trim_end().ends_with(')') {
                let value_parts = match shlex::split(value.as_str()) {
                    Some(value_parts) => value_parts,
                    None => {
                        error!("Failed to split value: {}", value.as_str());
                        continue;
                    }
                };
                variables.insert(key, value_parts);
            } else {
                keep = Some((key, value));
            }
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            if value.starts_with('(') {
                if value.trim_end().ends_with(')') {
                    let value = &value[1..value.len() - 1];
                    let value_parts = match shlex::split(value) {
                        Some(value_parts) => value_parts,
                        None => {
                            error!("Failed to split value: {}", value);
                            continue;
                        }
                    };
                    variables.insert(key.to_owned(), value_parts);
                } else {
                    keep = Some((key.to_owned(), value[1..].to_owned()));
                }
            } else {
                let value_parts = match shlex::split(value) {
                    Some(value_parts) => value_parts,
                    None => {
                        error!("Failed to split value: {}", value);
                        continue;
                    }
                };
                variables.insert(key.to_owned(), value_parts);
            }
        }
    }

    variables
}

pub fn guess_from_aur(package: &str) -> Vec<UpstreamDatum> {
    let mut variables = HashMap::new();

    for vcs in vcs::VCSES {
        let url = format!(
            "https://aur.archlinux.org/cgit/aur.git/plain/PKGBUILD?h={}-{}",
            package, vcs
        );
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::USER_AGENT, USER_AGENT.parse().unwrap());
        let client = reqwest::blocking::Client::builder()
            .default_headers(headers)
            .build()
            .unwrap();

        debug!("Requesting {}", url);
        let response = client.get(&url).send();

        match response {
            Ok(response) => {
                if response.status().is_success() {
                    let text = response.text().unwrap();
                    variables = parse_pkgbuild_variables(&text);
                    break;
                } else if response.status().as_u16() != 404 {
                    // If the response is not 404, raise an error
                    // response.error_for_status();
                    error!("Error contacting AUR: {}", response.status());
                    return Vec::new();
                } else {
                    continue;
                }
            }
            Err(e) => {
                error!("Error contacting AUR: {}", e);
                return Vec::new();
            }
        }
    }

    let mut results = Vec::new();

    for (key, value) in variables.iter() {
        match key.as_str() {
            "url" => {
                results.push(UpstreamDatum::Homepage(value[0].to_owned()));
            }
            "source" => {
                if value.is_empty() {
                    continue;
                }
                let mut value = value[0].to_owned();
                if value.contains("${") {
                    for (k, v) in variables.iter() {
                        value = value.replace(format!("${{{}}}", k).as_str(), v.join(" ").as_str());
                        value = value.replace(format!("${}", k).as_str(), v.join(" ").as_str());
                    }
                }
                let url = match value.split_once("::") {
                    Some((_unique_name, url)) => url,
                    None => value.as_str(),
                };
                let url = url.replace("#branch=", ",branch=");
                results.push(UpstreamDatum::Repository(
                    vcs::strip_vcs_prefixes(url.as_str()).to_owned(),
                ));
            }
            "_gitroot" => {
                results.push(UpstreamDatum::Repository(
                    vcs::strip_vcs_prefixes(value[0].as_str()).to_owned(),
                ));
            }
            _ => {
                debug!("Ignoring variable: {}", key);
            }
        }
    }

    results
}
