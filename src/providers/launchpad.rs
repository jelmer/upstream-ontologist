use crate::{load_json_url, UpstreamDatum};
use log::{error, warn};

/// Helper to fetch and parse JSON from a Launchpad API URL string.
async fn fetch_json(url_str: &str) -> Option<serde_json::Value> {
    let url = match url::Url::parse(url_str) {
        Ok(url) => url,
        Err(e) => {
            warn!("Launchpad: failed to parse URL {:?}: {}", url_str, e);
            return None;
        }
    };
    match load_json_url(&url, None).await {
        Ok(data) => Some(data),
        Err(e) => {
            warn!("Launchpad: failed to fetch {}: {}", url_str, e);
            None
        }
    }
}

/// Fetches upstream metadata from Launchpad
#[cfg(feature = "launchpad")]
pub async fn guess_from_launchpad(
    package: &str,
    distribution: Option<&str>,
    suite: Option<&str>,
) -> Option<Vec<UpstreamDatum>> {
    use distro_info::DistroInfo;
    use distro_info::UbuntuDistroInfo;
    let distribution = distribution.unwrap_or("ubuntu");
    let suite = suite.map_or_else(
        || {
            if distribution == "ubuntu" {
                let ubuntu = UbuntuDistroInfo::new().ok()?;
                Some(
                    ubuntu
                        .ubuntu_devel(chrono::Utc::now().date_naive())
                        .last()?
                        .codename()
                        .clone(),
                )
            } else if distribution == "debian" {
                Some("sid".to_string())
            } else {
                None
            }
        },
        |x| Some(x.to_string()),
    );

    let suite = suite?;

    let sourcepackage_url = format!(
        "https://api.launchpad.net/devel/{}/{}/+source/{}",
        distribution, suite, package
    );
    let sourcepackage_data = fetch_json(&sourcepackage_url).await?;

    let productseries_url = sourcepackage_data.get("productseries_link")?;
    let productseries_data = fetch_json(productseries_url.as_str()?).await?;

    let project_link = productseries_data.get("project_link")?.as_str()?;
    let project_data = fetch_json(project_link).await?;

    let mut results = Vec::new();

    if let Some(v) = project_data.get("homepage_url").and_then(|v| v.as_str()) {
        results.push(UpstreamDatum::Homepage(v.to_string()));
    }
    if let Some(v) = project_data.get("display_name").and_then(|v| v.as_str()) {
        results.push(UpstreamDatum::Name(v.to_string()));
    }
    if let Some(v) = project_data
        .get("sourceforge_project")
        .and_then(|v| v.as_str())
    {
        results.push(UpstreamDatum::SourceForgeProject(v.to_string()));
    }
    if let Some(v) = project_data.get("wiki_url").and_then(|v| v.as_str()) {
        results.push(UpstreamDatum::Wiki(v.to_string()));
    }
    if let Some(v) = project_data.get("summary").and_then(|v| v.as_str()) {
        results.push(UpstreamDatum::Summary(v.to_string()));
    }
    if let Some(v) = project_data.get("download_url").and_then(|v| v.as_str()) {
        results.push(UpstreamDatum::Download(v.to_string()));
    }

    let vcs = match project_data.get("vcs") {
        Some(vcs) => vcs,
        None => return Some(results),
    };

    let is_official = project_data.get("official_codehosting") == Some(&serde_json::json!("true"));

    if vcs == "Bazaar" {
        guess_launchpad_bzr_repo(&mut results, &productseries_data, is_official).await;
    } else if vcs == "Git" {
        let name = project_data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(package);
        guess_launchpad_git_repo(&mut results, name, is_official).await;
    } else {
        error!("unknown vcs: {:?}", vcs);
    }

    Some(results)
}

#[cfg(feature = "launchpad")]
async fn guess_launchpad_bzr_repo(
    results: &mut Vec<UpstreamDatum>,
    productseries_data: &serde_json::Value,
    is_official: bool,
) {
    let branch_link = match productseries_data
        .get("branch_link")
        .and_then(|v| v.as_str())
    {
        Some(link) => link,
        None => return,
    };

    let code_import_url = format!("{}/+code-import", branch_link);
    if let Some(code_import_data) = fetch_json(&code_import_url).await {
        if let Some(url) = code_import_data.get("url").and_then(|v| v.as_str()) {
            results.push(UpstreamDatum::Repository(url.to_string()));
            return;
        }
    }

    if !is_official {
        return;
    }
    let Some(branch_data) = fetch_json(branch_link).await else {
        return;
    };
    if let Some(v) = branch_data.get("bzr_identity").and_then(|v| v.as_str()) {
        results.push(UpstreamDatum::Repository(v.to_owned()));
    }
    if let Some(v) = branch_data.get("web_link").and_then(|v| v.as_str()) {
        results.push(UpstreamDatum::RepositoryBrowse(v.to_owned()));
    }
}

#[cfg(feature = "launchpad")]
async fn guess_launchpad_git_repo(
    results: &mut Vec<UpstreamDatum>,
    project_name: &str,
    is_official: bool,
) {
    let repo_link = format!(
        "https://api.launchpad.net/devel/+git?ws.op=getByPath&path={}",
        project_name
    );
    let Some(repo_data) = fetch_json(&repo_link).await else {
        return;
    };

    if let Some(code_import_link) = repo_data.get("code_import_link").and_then(|v| v.as_str()) {
        if let Some(code_import_data) = fetch_json(code_import_link).await {
            if let Some(url) = code_import_data.get("url").and_then(|v| v.as_str()) {
                results.push(UpstreamDatum::Repository(url.to_owned()));
                return;
            }
        }
    }

    if !is_official {
        return;
    }
    if let Some(v) = repo_data.get("git_https_url").and_then(|v| v.as_str()) {
        results.push(UpstreamDatum::Repository(v.to_owned()));
    }
    if let Some(v) = repo_data.get("web_link").and_then(|v| v.as_str()) {
        results.push(UpstreamDatum::RepositoryBrowse(v.to_owned()));
    }
}
