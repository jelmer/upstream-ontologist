use crate::with_path_segments;
use lazy_regex::regex;
use log::{debug, warn};
use pyo3::prelude::*;
use std::borrow::Cow;
use std::collections::HashMap;
use url::Url;

pub const VCSES: [&str; 3] = ["git", "bzr", "hg"];

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

pub fn plausible_browse_url(url: &str) -> bool {
    if let Ok(url) = url::Url::parse(url) {
        if url.scheme() == "https" || url.scheme() == "http" {
            return true;
        }
    }
    false
}

pub fn strip_vcs_prefixes(url: &str) -> &str {
    let prefixes = ["git", "hg"];

    for prefix in prefixes.iter() {
        if url.starts_with(&format!("{}+", prefix)) {
            return &url[prefix.len() + 1..];
        }
    }

    url
}

fn probe_upstream_github_branch_url(url: &url::Url, version: Option<&str>) -> Option<bool> {
    let path = url.path();
    let path = path.strip_suffix(".git").unwrap_or(path);
    let api_url = url::Url::parse(
        format!(
            "https://api.github.com/repos/{}/tags",
            path.trim_start_matches('/')
        )
        .as_str(),
    )
    .unwrap();
    match crate::load_json_url(&api_url, None) {
        Ok(json) => {
            if let Some(version) = version {
                let tags = json.as_array()?;
                let tag_names = tags
                    .iter()
                    .map(|x| x["name"].as_str().unwrap())
                    .collect::<Vec<_>>();
                if tag_names.is_empty() {
                    // Uhm, hmm
                    return Some(true);
                }
                return Some(version_in_tags(version, tag_names.as_slice()));
            }
            Some(true)
        }
        Err(crate::HTTPJSONError::Error { status, .. }) if status == 404 => Some(false),
        Err(crate::HTTPJSONError::Error { status, .. }) if status == 403 => {
            debug!("github api rate limit exceeded");
            None
        }
        Err(e) => {
            warn!("failed to probe github api: {:?}", e);
            None
        }
    }
}

fn version_in_tags(version: &str, tag_names: &[&str]) -> bool {
    if tag_names.contains(&version) {
        return true;
    }
    if tag_names.contains(&format!("v{}", version).as_str()) {
        return true;
    }
    if tag_names.contains(&format!("release/{}", version).as_str()) {
        return true;
    }
    if tag_names.contains(&version.replace('.', "_").as_str()) {
        return true;
    }
    for tag_name in tag_names {
        if tag_name.ends_with(&format!("_{}", version)) {
            return true;
        }
        if tag_name.ends_with(&format!("-{}", version)) {
            return true;
        }
        if tag_name.ends_with(&format!("_{}", version.replace('.', "_"))) {
            return true;
        }
    }
    false
}

fn probe_upstream_breezy_branch_url(url: &url::Url, version: Option<&str>) -> Option<bool> {
    let tags: HashMap<String, Vec<u8>> = Python::with_gil(|py| {
        let breezy_ui = py.import("breezy.ui")?;
        let branch_mod = py.import("breezy.branch")?;
        py.import("breezy.bzr")?;
        py.import("breezy.git")?;
        let old_ui = breezy_ui.getattr("ui_factory")?;
        breezy_ui.setattr("ui_factory", breezy_ui.call_method0("SilentUIFactory")?)?;
        let branch_cls = branch_mod.getattr("Branch")?;
        let branch = branch_cls.call_method1("open", (url.as_str(),))?;
        branch.call_method0("last_revision")?;
        let tags = branch.getattr("tags")?.call_method0("get_tag_dict")?;
        breezy_ui.setattr("ui_factory", old_ui)?;
        Ok::<HashMap<_, _>, PyErr>(tags.extract()?)
    })
    .map_err(|e| {
        warn!("failed to probe breezy branch: {:?}", e);
        e
    })
    .ok()?;

    let tag_names = tags.keys().map(|x| x.as_str()).collect::<Vec<_>>();
    if let Some(version) = version {
        Some(version_in_tags(version, tag_names.as_slice()))
    } else {
        Some(true)
    }
}

pub fn probe_upstream_branch_url(url: &url::Url, version: Option<&str>) -> Option<bool> {
    if url.scheme() == "git+ssh" || url.scheme() == "ssh" || url.scheme() == "bzr+ssh" {
        // Let's not probe anything possibly non-public.
        return None;
    }

    if url.host() == Some(url::Host::Domain("github.com")) {
        probe_upstream_github_branch_url(&url, version)
    } else {
        probe_upstream_breezy_branch_url(&url, version)
    }
}

pub fn check_repository_url_canonical(
    mut url: url::Url,
    version: Option<&str>,
) -> std::result::Result<url::Url, crate::CanonicalizeError> {
    if url.host_str() == Some("github.com") {
        let mut segments = url.path_segments().unwrap().collect::<Vec<_>>();
        if segments.len() < 2 {
            return Err(crate::CanonicalizeError::InvalidUrl(
                url,
                "GitHub URL with less than 2 path elements".to_string(),
            ));
        }

        if segments[0] == "sponsors" {
            return Err(crate::CanonicalizeError::InvalidUrl(
                url,
                "GitHub sponsors URL".to_string(),
            ));
        }

        segments[1] = segments[1].trim_end_matches(".git");
        let api_url = format!(
            "https://api.github.com/repos/{}/{}",
            segments[0], segments[1]
        );
        url = match crate::load_json_url(&url::Url::parse(api_url.as_str()).unwrap(), None) {
            Ok(data) => {
                if data["archived"].as_bool().unwrap_or(false) {
                    return Err(crate::CanonicalizeError::InvalidUrl(
                        url,
                        "GitHub URL is archived".to_string(),
                    ));
                }

                if let Some(description) = data["description"].as_str() {
                    if description.contains("DEPRECATED") {
                        return Err(crate::CanonicalizeError::InvalidUrl(
                            url,
                            "GitHub URL is deprecated".to_string(),
                        ));
                    }

                    if description.starts_with("Moved to") {
                        let url = url::Url::parse(
                            description
                                .trim_start_matches("Moved to ")
                                .trim_end_matches("."),
                        )
                        .unwrap();
                        return check_repository_url_canonical(url, version);
                    }

                    if description.contains("has moved") {
                        return Err(crate::CanonicalizeError::InvalidUrl(
                            url,
                            "GitHub URL has moved".to_string(),
                        ));
                    }

                    if description.starts_with("Mirror of ") {
                        let url = url::Url::parse(
                            description
                                .trim_start_matches("Mirror of ")
                                .trim_end_matches("."),
                        )
                        .unwrap();
                        return check_repository_url_canonical(url, version);
                    }
                }

                if let Some(homepage) = data["homepage"].as_str() {
                    if is_gitlab_site(homepage, None) {
                        return Err(crate::CanonicalizeError::InvalidUrl(
                            url,
                            format!("homepage is on GitLab: {}", homepage),
                        ));
                    }
                }

                // TODO(jelmer): Look at the contents of the repository; if it contains just a
                // single README file with < 10 lines, assume the worst.
                // return data['clone_url']

                Ok(url::Url::parse(data["clone_url"].as_str().unwrap()).unwrap())
            }
            Err(crate::HTTPJSONError::Error { status, .. }) if status == 404 => {
                return Err(crate::CanonicalizeError::InvalidUrl(
                    url,
                    "GitHub URL does not exist".to_string(),
                ))
            }
            Err(crate::HTTPJSONError::Error { status, .. }) if status == 403 => {
                return Err(crate::CanonicalizeError::Unverifiable(
                    url,
                    "GitHub URL rate-limited".to_string(),
                ))
            }
            Err(e) => {
                return Err(crate::CanonicalizeError::Unverifiable(
                    url,
                    format!("GitHub URL failed to load: {:?}", e),
                ))
            }
        }?;
    }

    let is_valid = probe_upstream_branch_url(&url, version);
    if is_valid.is_none() {
        return Err(crate::CanonicalizeError::Unverifiable(
            url,
            "unable to probe".to_string(),
        ));
    }

    if is_valid.unwrap() {
        return Ok(url);
    }

    Err(crate::CanonicalizeError::InvalidUrl(
        url,
        "unable to successfully probe URL".to_string(),
    ))
}

const KNOWN_GITLAB_SITES: &[&str] = &["salsa.debian.org", "invent.kde.org", "0xacab.org"];

const KNOWN_HOSTING_SITES: &[&str] = &[
    "code.launchpad.net",
    "github.com",
    "launchpad.net",
    "git.openstack.org",
];

pub fn is_gitlab_site(hostname: &str, net_access: Option<bool>) -> bool {
    if KNOWN_GITLAB_SITES.contains(&hostname) {
        return true;
    }

    if hostname.starts_with("gitlab.") {
        return true;
    }

    if net_access.unwrap_or(false) {
        probe_gitlab_host(hostname)
    } else {
        false
    }
}

pub fn probe_gitlab_host(hostname: &str) -> bool {
    let url = format!("https://{}/api/v4/version", hostname);
    match crate::load_json_url(&url::Url::parse(url.as_str()).unwrap(), None) {
        Ok(data) => true,
        Err(crate::HTTPJSONError::Error {
            status, response, ..
        }) if status == 401 => {
            if let Ok(data) = response.json::<serde_json::Value>() {
                if let Some(message) = data["message"].as_str() {
                    if message == "401 Unauthorized" {
                        true
                    } else {
                        debug!("failed to parse JSON response: {:?}", data);
                        false
                    }
                } else {
                    debug!("failed to parse JSON response: {:?}", data);
                    false
                }
            } else {
                debug!("failed to parse JSON response");
                false
            }
        }
        Err(e) => {
            debug!("failed to probe GitLab host: {:?}", e);
            false
        }
    }
}

pub fn guess_repo_from_url(url: &url::Url, net_access: Option<bool>) -> Option<String> {
    let net_access = net_access.unwrap_or(false);
    let path_segments = url.path_segments().unwrap().collect::<Vec<_>>();
    match url.host_str()? {
        "github.com" => {
            if path_segments.len() < 2 {
                return None;
            }

            Some(
                with_path_segments(url, &path_segments[0..2])
                    .unwrap()
                    .to_string(),
            )
        }
        "travis-ci.org" => {
            if path_segments.len() < 2 {
                return None;
            }

            Some(format!(
                "https://github.com/{}/{}",
                path_segments[0], path_segments[1]
            ))
        }
        "coveralls.io" => {
            if path_segments.len() < 3 {
                return None;
            }
            if path_segments[0] != "r" {
                return None;
            }
            Some(format!(
                "https://github.com/{}/{}",
                path_segments[1], path_segments[2]
            ))
        }
        "launchpad.net" => Some(
            url::Url::parse(format!("https://code.launchpad.net/{}", path_segments[0]).as_str())
                .unwrap()
                .to_string(),
        ),
        "git.savannah.gnu.org" => {
            if path_segments.len() < 2 {
                return None;
            }
            if path_segments[0] != "git" {
                return None;
            }
            Some(url.to_string())
        }
        "freedesktop.org" | "www.freedesktop.org" => {
            if path_segments.len() >= 2 && path_segments[0] == "software" {
                Some(
                    url::Url::parse(
                        format!("https://github.com/freedesktop/{}", path_segments[1]).as_str(),
                    )
                    .unwrap()
                    .to_string(),
                )
            } else if path_segments.len() >= 3 && path_segments[0..2] == ["wiki", "Software"] {
                Some(
                    url::Url::parse(
                        format!("https://github.com/freedesktop/{}", path_segments[2]).as_str(),
                    )
                    .unwrap()
                    .to_string(),
                )
            } else {
                None
            }
        }
        "download.gnome.org" => {
            if path_segments.len() < 2 {
                return None;
            }
            if path_segments[0] != "sources" {
                return None;
            }
            Some(
                url::Url::parse(
                    format!("https://gitlab.gnome.org/GNOME/{}.git", path_segments[1]).as_str(),
                )
                .unwrap()
                .to_string(),
            )
        }
        "download.kde.org" => {
            if path_segments.len() < 2 {
                return None;
            }
            if path_segments[0] != "stable" && path_segments[0] != "unstable" {
                return None;
            }
            Some(
                url::Url::parse(format!("https://invent.kde.org/{}", path_segments[1]).as_str())
                    .unwrap()
                    .to_string(),
            )
        }
        "ftp.gnome.org" => {
            if path_segments.len() >= 4
                && path_segments[0] == "pub"
                && path_segments[1] == "GNOME"
                && path_segments[2] == "sources"
            {
                Some(
                    url::Url::parse(
                        format!("https://gitlab.gnome.org/GNOME/{}.git", path_segments[3]).as_str(),
                    )
                    .unwrap()
                    .to_string(),
                )
            } else {
                None
            }
        }
        "sourceforge.net" => {
            if path_segments.len() >= 4 && path_segments[0] == "p" && path_segments[3] == "ci" {
                Some(
                    url::Url::parse(
                        format!(
                            "https://sourceforge.net/p/{}/{}",
                            path_segments[1], path_segments[2]
                        )
                        .as_str(),
                    )
                    .unwrap()
                    .to_string(),
                )
            } else {
                None
            }
        }
        "www.apache.org" => {
            if path_segments.len() >= 2 && path_segments[0] == "dist" {
                Some(
                    url::Url::parse(
                        format!("https://svn.apache.org/repos/asf/{}", path_segments[1]).as_str(),
                    )
                    .unwrap()
                    .to_string(),
                )
            } else {
                None
            }
        }
        "bitbucket.org" => {
            if path_segments.len() < 2 {
                return None;
            }

            Some(
                with_path_segments(url, &path_segments[0..2])
                    .unwrap()
                    .to_string(),
            )
        }
        "ftp.gnu.org" => {
            if path_segments.len() < 2 {
                return None;
            }
            if path_segments[0] != "gnu" {
                return None;
            }
            Some(
                url::Url::parse(
                    format!("https://git.savannah.gnu.org/git/{}", path_segments[1]).as_str(),
                )
                .unwrap()
                .to_string(),
            )
        }
        "download.savannah.gnu.org" => {
            if path_segments.len() < 2 {
                return None;
            }
            if path_segments[0] != "releases" {
                return None;
            }
            Some(
                url::Url::parse(
                    format!("https://git.savannah.gnu.org/git/{}", path_segments[1]).as_str(),
                )
                .unwrap()
                .to_string(),
            )
        }
        u if is_gitlab_site(u, Some(net_access)) => {
            if path_segments.len() < 1 {
                return None;
            }
            let proj_segments = if path_segments.contains(&"-") {
                path_segments[0..path_segments.iter().position(|s| s.contains("-")).unwrap()]
                    .to_vec()
            } else if path_segments.contains(&"tags") {
                path_segments[0..path_segments.iter().position(|s| s == &"tags").unwrap()].to_vec()
            } else if path_segments.contains(&"blob") {
                path_segments[0..path_segments.iter().position(|s| s == &"blob").unwrap()].to_vec()
            } else {
                path_segments.to_vec()
            };

            Some(with_path_segments(url, &proj_segments).unwrap().to_string())
        }
        "git.php.net" => {
            if path_segments[0] == "repository" {
                Some(url.to_string())
            } else if path_segments.len() == 0 {
                let qs = url.query_pairs().collect::<HashMap<_, _>>();
                qs.get("p")
                    .map(|p| {
                        url::Url::parse(format!("https://git.php.net/repository/?{}", p).as_str())
                            .unwrap()
                    })
                    .map(|u| u.to_string())
            } else {
                None
            }
        }
        u if KNOWN_HOSTING_SITES.contains(&u) => Some(url.to_string()),
        u if u.starts_with("svn.") => {
            // 'svn' subdomains are often used for hosting SVN repositories
            Some(url.to_string())
        }
        _ => {
            if net_access {
                match check_repository_url_canonical(url.clone(), None) {
                    Ok(url) => Some(url.to_string()),
                    Err(_) => {
                        debug!("Failed to canonicalize URL: {}", url);
                        None
                    }
                }
            } else {
                None
            }
        }
    }
}

pub fn canonical_git_repo_url(repo_url: &Url, net_access: Option<bool>) -> Url {
    if let Some(hostname) = repo_url.host_str() {
        if (is_gitlab_site(hostname, net_access) || hostname == "github.com")
            && !repo_url.path().ends_with(".git")
        {
            let mut url = repo_url.clone();
            url.set_path(&(url.path().to_owned() + ".git"));
            return url;
        }
    }
    repo_url.clone()
}

pub fn browse_url_from_repo_url(
    url: &str,
    branch: Option<&str>,
    subpath: Option<&str>,
    net_access: Option<bool>,
) -> Option<String> {
    if let Ok(parsed_url) = Url::parse(url) {
        if parsed_url.host_str() == Some("github.com") {
            let mut path = parsed_url
                .path_segments()
                .unwrap()
                .take(3)
                .collect::<Vec<&str>>()
                .join("/");
            if path.ends_with(".git") {
                path = path[..path.len() - 4].to_string();
            }
            if subpath.is_some() || branch.is_some() {
                path.push_str(&format!("/tree/{}", branch.unwrap_or("HEAD")));
            }
            if let Some(subpath_str) = subpath {
                path.push_str(&format!("/{}", subpath_str));
            }
            return Some(
                Url::parse("https://github.com")
                    .unwrap()
                    .join(&path)
                    .unwrap()
                    .to_string(),
            );
        } else if parsed_url.host_str() == Some("gopkg.in") {
            let mut els = parsed_url
                .path_segments()
                .unwrap()
                .take(3)
                .collect::<Vec<&str>>();
            if els.len() != 2 {
                return None;
            }
            if let Some(version) = els[2].strip_prefix(".v") {
                els[2] = "";
                let mut path = els.join("/");
                path.push_str(&format!("/tree/{}", version));
                if let Some(subpath_str) = subpath {
                    path.push_str(&format!("/{}", subpath_str));
                }
                return Some(
                    Url::parse("https://github.com")
                        .unwrap()
                        .join(&path)
                        .unwrap()
                        .to_string(),
                );
            }
        } else if parsed_url.host_str() == Some("code.launchpad.net")
            || parsed_url.host_str() == Some("launchpad.net")
        {
            let mut path = parsed_url.path().to_string();
            if let Some(subpath_str) = subpath {
                path.push_str(&format!("/view/head:{}", subpath_str));
                return Some(
                    Url::parse(format!("https://bazaar.launchpad.net{}", path).as_str())
                        .unwrap()
                        .to_string(),
                );
            } else {
                return Some(
                    Url::parse(format!("https://code.launchpad.net{}", path).as_str())
                        .unwrap()
                        .to_string(),
                );
            }
        } else if parsed_url.host_str() == Some("svn.apache.org") {
            let path_elements = parsed_url
                .path_segments()
                .map(|segments| segments.into_iter().collect::<Vec<&str>>())
                .unwrap_or_else(Vec::new);
            if path_elements.len() >= 2 && path_elements[0] == "repos" && path_elements[1] == "asf"
            {
                let mut path_elements = path_elements.into_iter().skip(1).collect::<Vec<&str>>();
                path_elements[0] = "viewvc";
                if let Some(subpath_str) = subpath {
                    path_elements.push(subpath_str);
                }
                return Some(
                    Url::parse(
                        format!("https://svn.apache.org{}", path_elements.join("/")).as_str(),
                    )
                    .unwrap()
                    .to_string(),
                );
            }
        } else if parsed_url.host_str() == Some("git.savannah.gnu.org")
            || parsed_url.host_str() == Some("git.sv.gnu.org")
        {
            let mut path_elements = parsed_url.path_segments().unwrap().collect::<Vec<&str>>();
            if parsed_url.scheme() == "https" && path_elements.first() == Some(&"git") {
                path_elements.remove(0);
            }
            path_elements.insert(0, "cgit");
            if let Some(subpath_str) = subpath {
                path_elements.push("tree");
                path_elements.push(subpath_str);
            }
            return Some(
                Url::parse(
                    format!("https://git.savannah.gnu.org{}", path_elements.join("/")).as_str(),
                )
                .unwrap()
                .to_string(),
            );
        } else if is_gitlab_site(parsed_url.host_str().unwrap(), net_access) {
            let mut path = parsed_url.path().to_string();
            if path.ends_with(".git") {
                path = path[..path.len() - 4].to_string();
            }
            if let Some(subpath_str) = subpath {
                path.push_str(&format!("/-/blob/HEAD/{}", subpath_str));
            }
            return Some(
                Url::parse(format!("https://{}{}", parsed_url.host_str().unwrap(), path).as_str())
                    .unwrap()
                    .to_string(),
            );
        }
    }
    None
}

pub fn find_public_repo_url(repo_url: &str, net_access: Option<bool>) -> Option<String> {
    let parsed = match Url::parse(repo_url) {
        Ok(parsed) => parsed,
        Err(_) => {
            if repo_url.contains(':') {
                let re = regex!(r"^(?P<user>[^@:/]+@)?(?P<host>[^/:]+):(?P<path>.*)$");
                if let Some(captures) = re.captures(repo_url) {
                    let host = captures.name("host").unwrap().as_str();
                    let path = captures.name("path").unwrap().as_str();
                    if host == "github.com" || is_gitlab_site(host, net_access) {
                        return Some(format!("https://{}/{}", host, path));
                    }
                }
            }
            return None;
        }
    };

    let revised_url: Option<String>;
    match parsed.host_str() {
        Some("github.com") => {
            if ["https", "http", "git"].contains(&parsed.scheme()) {
                return Some(repo_url.to_string());
            }
            revised_url = Some(
                Url::parse("https://github.com")
                    .unwrap()
                    .join(parsed.path())
                    .unwrap()
                    .to_string(),
            );
        }
        Some(hostname) if is_gitlab_site(hostname, net_access) => {
            if ["https", "http"].contains(&parsed.scheme()) {
                return Some(repo_url.to_string());
            }
            if parsed.scheme() == "ssh" {
                revised_url = Some(format!(
                    "https://{}{}",
                    parsed.host_str().unwrap(),
                    parsed.path(),
                ));
            } else {
                revised_url = None;
            }
        }
        Some("code.launchpad.net") | Some("bazaar.launchpad.net") | Some("git.launchpad.net") => {
            if parsed.scheme().starts_with("http") || parsed.scheme() == "lp" {
                return Some(repo_url.to_string());
            }
            if ["ssh", "bzr+ssh"].contains(&parsed.scheme()) {
                revised_url = Some(format!(
                    "https://{}{}",
                    parsed.host_str().unwrap(),
                    parsed.path()
                ));
            } else {
                revised_url = None;
            }
        }
        _ => revised_url = None,
    }

    revised_url
}
