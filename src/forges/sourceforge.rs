use crate::UpstreamDatum;
use log::{debug, error, warn};
use crate::{load_json_url, HTTPJSONError};
use crate::check_bug_database_canonical;
use crate::USER_AGENT;
use reqwest::Url;
use lazy_regex::regex;

fn sf_git_extract_url(page: &str) -> Option<String> {
    let document = select::document::Document::from(page);
    use select::predicate::Attr;

    let el = document.find(Attr("id", "access_url")).next()?;

    let value = el.attr("value").unwrap();
    let access_command: Vec<&str> = value.split(' ').collect();
    if access_command.len() < 3 || access_command[..2] != ["git", "clone"] {
        return None;
    }

    Some(access_command[2].to_string())
}

pub fn get_sf_metadata(project: &str) -> Option<serde_json::Value> {
    let url = format!("https://sourceforge.net/rest/p/{}", project);
    match load_json_url(&Url::parse(url.as_str()).unwrap(), None) {
        Ok(data) => Some(data),
        Err(HTTPJSONError::Error { status, .. }) if status == reqwest::StatusCode::NOT_FOUND => {
            None
        }
        r => panic!("Unexpected result from {}: {:?}", url, r),
    }
}

fn parse_sf_json(data: serde_json::Value, project: &str, subproject: Option<&str>) -> Vec<UpstreamDatum> {
    let mut results = Vec::new();
    if let Some(name) = data.get("name").and_then(|name| name.as_str()) {
        results.push(UpstreamDatum::Name(name.to_string()));
    }
    if let Some(external_homepage) = data.get("external_homepage").and_then(|url| url.as_str()){
        results.push(UpstreamDatum::Homepage(external_homepage.to_string()));
    }
    if let Some(preferred_support_url) = data.get("preferred_support_url").and_then(|url| url.as_str()).filter(|x| !x.is_empty()) {
        let preferred_support_url = Url::parse(preferred_support_url)
            .expect("preferred_support_url is not a valid URL");
        match check_bug_database_canonical(&preferred_support_url, Some(true)) {
            Ok(canonical_url) => {
                results.push(UpstreamDatum::BugDatabase(canonical_url.to_string()));
            }
            Err(_) => {
                results.push(UpstreamDatum::BugDatabase(
                    preferred_support_url.to_string(),
                ));
            }
        }
    }

    let vcs_names = ["hg", "git", "svn", "cvs", "bzr"];
    let mut vcs_tools: Vec<(&str, Option<&str>, &str)> =
        data.get("tools").map_or_else(Vec::new, |tools| {
            tools
                .as_array()
                .unwrap()
                .iter()
                .filter(|tool| {
                    vcs_names.contains(&tool.get("name").unwrap().as_str().unwrap())
                })
                .map(|tool| {
                    (
                        tool.get("name").map_or("", |n| n.as_str().unwrap()),
                        tool.get("mount_label").map(|l| l.as_str().unwrap()),
                        tool.get("url").map_or("", |u| u.as_str().unwrap()),
                    )
                })
                .collect::<Vec<(&str, Option<&str>, &str)>>()
        });

    if vcs_tools.len() > 1 {
        vcs_tools.retain(|tool| {
            if let Some(url) = tool.2.strip_suffix('/') {
                !["www", "homepage"].contains(&url.rsplit('/').next().unwrap_or(""))
            } else {
                true
            }
        });
    }

    if vcs_tools.len() > 1 && subproject.is_some() {
        let new_vcs_tools: Vec<(&str, Option<&str>, &str)> = vcs_tools
            .iter()
            .filter(|tool| tool.1 == subproject)
            .cloned()
            .collect();
        if !new_vcs_tools.is_empty() {
            vcs_tools = new_vcs_tools;
        }
    }

    if vcs_tools.iter().any(|tool| tool.0 == "cvs") {
        vcs_tools.retain(|tool| tool.0 != "cvs");
    }

    if vcs_tools.len() == 1 {
        let (kind, _, url) = vcs_tools[0];
        match kind {
            "git" => {
                let url = format!("https://sourceforge.net/{}", url);
                let client = reqwest::blocking::Client::new();
                let response = client
                    .head(url)
                    .header("User-Agent", USER_AGENT)
                    .send()
                    .unwrap();
                let url = sf_git_extract_url(&response.text().unwrap());
                if let Some(url) = url {
                    results.push(UpstreamDatum::Repository(url));
                }
            }
            "svn" => {
                let url = Url::parse("https://svn.code.sf.net/{}").unwrap().join(url).unwrap();
                results.push(UpstreamDatum::Repository(url.to_string()));
            }
            "hg" => {
                let url = format!("https://hg.code.sf.net/{}", url);
                results.push(UpstreamDatum::Repository(url));
            }
            "cvs" => {
                let url = format!(
                    "cvs+pserver://anonymous@{}.cvs.sourceforge.net/cvsroot/{}",
                    project,
                    url.strip_suffix('/')
                        .unwrap_or("")
                        .rsplit('/')
                        .nth(1)
                        .unwrap_or("")
                );
                results.push(UpstreamDatum::Repository(url));
            }
            "bzr" => {
                // TODO: Implement Bazaar (BZR) handling
            }
            _ => {
                error!("Unknown VCS kind: {}", kind);
            }
        }
    } else if vcs_tools.len() > 1 {
        warn!("Multiple possible VCS URLs found");
    }
    results
}

pub fn guess_from_sf(sf_project: &str, subproject: Option<&str>) -> Vec<UpstreamDatum> {
    let mut results = Vec::new();
    match get_sf_metadata(sf_project) {
        Some(data) => {
            results.extend(parse_sf_json(data, sf_project, subproject));
        }
        None => {
            debug!("No SourceForge metadata found for {}", sf_project);
        }
    }
    results
}

pub fn extract_sf_project_name(url: &str) -> Option<String> {
    let projects_regex = regex!(r"https?://sourceforge\.net/(projects|p)/([^/]+)");
    if let Some(captures) = projects_regex.captures(url) {
        return captures.get(2).map(|m| m.as_str().to_string());
    }

    let sf_regex = regex!(r"https?://(.*).(sf|sourceforge).(net|io)/.*");
    if let Some(captures) = sf_regex.captures(url) {
        return captures.get(1).map(|m| m.as_str().to_string());
    }

    None
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_parse_sf_json() {
        let data: serde_json::Value = serde_json::from_str(include_str!("../testdata/gtab.json")).unwrap();
        assert_eq!(parse_sf_json(data, "gtab", Some("gtab")), vec![
            UpstreamDatum::Name("gtab".to_string()),
            UpstreamDatum::Homepage("http://gtab.sourceforge.net".to_string()),
            UpstreamDatum::Repository("https://sourceforge.net/p/gtab/svn/".to_string()),
        ]);
    }
}
