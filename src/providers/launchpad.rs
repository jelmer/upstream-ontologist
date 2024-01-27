use crate::{load_json_url, UpstreamDatum};
use log::error;

#[cfg(feature = "launchpad")]
pub fn guess_from_launchpad(
    package: &str,
    distribution: Option<&str>,
    suite: Option<&str>,
) -> Option<Vec<UpstreamDatum>> {
    use distro_info::UbuntuDistroInfo;
    let distribution = distribution.unwrap_or("ubuntu");
    let suite = suite.map_or_else(
        || {
            if distribution == "ubuntu" {
                let ubuntu = UbuntuDistroInfo::new().unwrap();
                Some(
                    ubuntu
                        .devel(chrono::Utc::now().date_naive())
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

    let sourcepackage_data =
        load_json_url(&url::Url::parse(sourcepackage_url.as_str()).unwrap(), None).unwrap();
    if let Some(productseries_url) = sourcepackage_data.get("productseries_link") {
        let productseries_data = load_json_url(
            &url::Url::parse(productseries_url.as_str().unwrap()).unwrap(),
            None,
        )
        .unwrap();
        let project_link = productseries_data.get("project_link").cloned();

        if let Some(project_link) = project_link {
            let project_data = load_json_url(
                &url::Url::parse(project_link.as_str().unwrap()).unwrap(),
                None,
            )
            .unwrap();
            let mut results = Vec::new();

            if let Some(homepage_url) = project_data.get("homepage_url") {
                results.push(UpstreamDatum::Homepage(
                    homepage_url.as_str().unwrap().to_string(),
                ));
            }

            if let Some(display_name) = project_data.get("display_name") {
                results.push(UpstreamDatum::Name(
                    display_name.as_str().unwrap().to_string(),
                ));
            }

            if let Some(sourceforge_project) = project_data.get("sourceforge_project") {
                results.push(UpstreamDatum::SourceForgeProject(
                    sourceforge_project.as_str().unwrap().to_string(),
                ));
            }

            if let Some(wiki_url) = project_data.get("wiki_url") {
                results.push(UpstreamDatum::Wiki(wiki_url.as_str().unwrap().to_string()));
            }

            if let Some(summary) = project_data.get("summary") {
                results.push(UpstreamDatum::Summary(
                    summary.as_str().unwrap().to_string(),
                ));
            }

            if let Some(download_url) = project_data.get("download_url") {
                results.push(UpstreamDatum::Download(
                    download_url.as_str().unwrap().to_string(),
                ));
            }

            if let Some(vcs) = project_data.get("vcs") {
                if vcs == "Bazaar" {
                    if let Some(branch_link) = productseries_data.get("branch_link") {
                        let code_import_data = load_json_url(
                            &url::Url::parse(
                                format!("{}/+code-import", branch_link.as_str().unwrap()).as_str(),
                            )
                            .unwrap(),
                            None,
                        )
                        .unwrap();
                        if let Some(url) = code_import_data.get("url") {
                            results
                                .push(UpstreamDatum::Repository(url.as_str().unwrap().to_string()));
                        }
                    } else if let Some(official_codehosting) =
                        project_data.get("official_codehosting")
                    {
                        if official_codehosting == "true" {
                            let branch_data = load_json_url(
                                &url::Url::parse(
                                    productseries_data.as_object().unwrap()["branch_link"]
                                        .as_str()
                                        .unwrap(),
                                )
                                .unwrap(),
                                None,
                            )
                            .unwrap();
                            results.push(UpstreamDatum::Repository(
                                branch_data.as_object().unwrap()["bzr_identity"]
                                    .as_str()
                                    .unwrap()
                                    .to_owned(),
                            ));
                            results.push(UpstreamDatum::RepositoryBrowse(
                                branch_data.as_object().unwrap()["web_link"]
                                    .as_str()
                                    .unwrap()
                                    .to_owned(),
                            ));
                        }
                    }
                } else if vcs == "Git" {
                    let repo_link = format!(
                        "https://api.launchpad.net/devel/+git?ws.op=getByPath&path={}",
                        project_data["name"]
                    );

                    let repo_data =
                        load_json_url(&url::Url::parse(repo_link.as_str()).unwrap(), None).unwrap();

                    if let Some(code_import_link) = repo_data.get("code_import_link") {
                        let code_import_data = load_json_url(
                            &url::Url::parse(code_import_link.as_str().unwrap()).unwrap(),
                            None,
                        )
                        .unwrap();

                        if let Some(url) = code_import_data.get("url") {
                            results
                                .push(UpstreamDatum::Repository(url.as_str().unwrap().to_owned()));
                        }
                    } else if let Some(official_codehosting) =
                        project_data.get("official_codehosting")
                    {
                        if official_codehosting == "true" {
                            results.push(UpstreamDatum::Repository(
                                repo_data["git_https_url"].as_str().unwrap().to_owned(),
                            ));
                            results.push(UpstreamDatum::RepositoryBrowse(
                                repo_data["web_link"].as_str().unwrap().to_owned(),
                            ));
                        }
                    }
                } else {
                    error!("unknown vcs: {:?}", vcs);
                }
            }

            return Some(results);
        }
    }

    None
}
