pub fn plausible_url(url: &str) -> bool {
    url.contains(':')
}

pub fn drop_vcs_in_scheme(mut url: &str) -> &str {
    if url.starts_with("git+http:") || url.starts_with("git+https:") {
        url = &url[4..];
    }
    if url.starts_with("hg+https:") || url.starts_with("hg+http:") {
        url = &url[3..];
    }
    if url.starts_with("bzr+lp:") || url.starts_with("bzr+http:") {
        url = url.split_once('+').map(|x| x.1).unwrap_or("");
    }
    url
}

pub fn unsplit_vcs_url(repo_url: &str, branch: Option<&str>, subpath: Option<&str>) -> String {
    let mut url = repo_url.to_string();
    if let Some(branch_name) = branch {
        url = format!("{} -b {}", url, branch_name);
    }
    if let Some(subpath_str) = subpath {
        url = format!("{} [{}]", url, subpath_str);
    }
    url
}
