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
