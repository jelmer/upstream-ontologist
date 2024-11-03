use crate::check_bug_database_canonical;
use crate::UpstreamDatum;
use crate::{load_json_url, HTTPJSONError};
use lazy_regex::regex;
use log::{debug, error, warn};
use reqwest::Url;

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

fn parse_sf_json(
    data: serde_json::Value,
    project: &str,
    subproject: Option<&str>,
) -> Vec<UpstreamDatum> {
    let mut results = Vec::new();
    if let Some(name) = data.get("name").and_then(|name| name.as_str()) {
        results.push(UpstreamDatum::Name(name.to_string()));
    }
    if let Some(external_homepage) = data.get("external_homepage").and_then(|url| url.as_str()) {
        results.push(UpstreamDatum::Homepage(external_homepage.to_string()));
    }
    if let Some(preferred_support_url) = data
        .get("preferred_support_url")
        .and_then(|url| url.as_str())
        .filter(|x| !x.is_empty())
    {
        let preferred_support_url =
            Url::parse(preferred_support_url).expect("preferred_support_url is not a valid URL");
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
    let mut vcs_tools = data.get("tools").map_or_else(Vec::new, |tools| {
        tools
            .as_array()
            .unwrap()
            .iter()
            .filter(|tool| vcs_names.contains(&tool.get("name").unwrap().as_str().unwrap()))
            .map(|tool| {
                (
                    tool.get("name").map_or("", |n| n.as_str().unwrap()),
                    tool.get("mount_label").map(|l| l.as_str().unwrap()),
                    tool.clone(),
                )
            })
            .collect::<Vec<(&str, Option<&str>, serde_json::Value)>>()
    });

    if vcs_tools.len() > 1 {
        vcs_tools.retain(|tool| {
            if let Some(url) = tool
                .2
                .get("url")
                .and_then(|x| x.as_str())
                .and_then(|url| url.strip_suffix('/'))
            {
                !["www", "web", "homepage"].contains(&url.rsplit('/').next().unwrap_or(""))
            } else {
                true
            }
        });
    }

    if vcs_tools.len() > 1 && subproject.is_some() {
        let new_vcs_tools = vcs_tools
            .iter()
            .filter(|tool| tool.1 == subproject)
            .cloned()
            .collect::<Vec<_>>();
        if !new_vcs_tools.is_empty() {
            vcs_tools = new_vcs_tools;
        }
    }

    if vcs_tools.iter().any(|tool| tool.0 == "cvs") {
        vcs_tools.retain(|tool| tool.0 != "cvs");
    }

    match vcs_tools.len().cmp(&1) {
        std::cmp::Ordering::Equal => {
            let (kind, _, data) = &vcs_tools[0];
            match *kind {
                "git" => {
                    if let Some(url) = data.get("clone_url_https_anon").and_then(|x| x.as_str()) {
                        results.push(UpstreamDatum::Repository(url.to_owned()));
                    }
                }
                "svn" => {
                    if let Some(url) = data.get("clone_url_https_anon").and_then(|x| x.as_str()) {
                        results.push(UpstreamDatum::Repository(url.to_owned()));
                    }
                }
                "hg" => {
                    if let Some(url) = data.get("clone_url_ro").and_then(|x| x.as_str()) {
                        results.push(UpstreamDatum::Repository(url.to_owned()));
                    }
                }
                "cvs" => {
                    let url = format!(
                        "cvs+pserver://anonymous@{}.cvs.sourceforge.net/cvsroot/{}",
                        project,
                        data.get("url")
                            .unwrap()
                            .as_str()
                            .unwrap()
                            .strip_suffix('/')
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
        }
        std::cmp::Ordering::Greater => {
            warn!("Multiple possible VCS URLs found");
        }
        _ => {}
    }
    results
}

pub async fn guess_from_sf(sf_project: &str, subproject: Option<&str>) -> Vec<UpstreamDatum> {
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
    fn test_parse_sf_json_svn() {
        // From https://sourceforge.net/rest/p/gtab
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/gtab.json")).unwrap();
        assert_eq!(
            parse_sf_json(data, "gtab", Some("gtab")),
            vec![
                UpstreamDatum::Name("gtab".to_string()),
                UpstreamDatum::Homepage("http://gtab.sourceforge.net".to_string()),
                UpstreamDatum::Repository("https://svn.code.sf.net/p/gtab/svn/trunk".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_sf_json_git() {
        // From https://sourceforge.net/rest/p/zsh
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/zsh.json")).unwrap();
        assert_eq!(
            parse_sf_json(data, "zsh", Some("zsh")),
            vec![
                UpstreamDatum::Name("zsh".to_string()),
                UpstreamDatum::Homepage("http://zsh.sourceforge.net/".to_string()),
                UpstreamDatum::Repository("https://git.code.sf.net/p/zsh/code".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_sf_json_hg_diff() {
        // From https://sourceforge.net/rest/p/hg-diff
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/hg-diff.json")).unwrap();
        assert_eq!(
            parse_sf_json(data, "hg-diff", Some("hg-diff")),
            vec![
                UpstreamDatum::Name("hg-diff".to_string()),
                UpstreamDatum::Homepage("http://hg-diff.sourceforge.net/".to_string()),
                UpstreamDatum::Repository("http://hg.code.sf.net/p/hg-diff/code".to_string())
            ]
        );
    }

    #[test]
    fn test_parse_sf_json_docdb_v() {
        // From https://sourceforge.net/rest/p/docdb-v
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/docdb-v.json")).unwrap();
        assert_eq!(
            parse_sf_json(data, "docdb-v", Some("docdb-v")),
            vec![
                UpstreamDatum::Name("DocDB".to_string()),
                UpstreamDatum::Homepage("http://docdb-v.sourceforge.net".to_string()),
                UpstreamDatum::BugDatabase(
                    "http://sourceforge.net/tracker/?func=add&group_id=164024&atid=830064"
                        .to_string()
                ),
                UpstreamDatum::Repository("https://git.code.sf.net/p/docdb-v/git".to_string())
            ]
        );
    }
}
