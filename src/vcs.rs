use crate::with_path_segments;
use lazy_regex::regex;
use log::{debug, warn};
use pyo3::prelude::*;
use std::borrow::Cow;

use std::collections::HashMap;
use url::Url;

pub const VCSES: &[&str] = &["git", "bzr", "hg"];

pub const KNOWN_GITLAB_SITES: &[&str] = &[
    "salsa.debian.org",
    "invent.kde.org",
    "0xacab.org",
];

pub const SECURE_SCHEMES: &[&str] = &["https", "git+ssh", "bzr+ssh", "hg+ssh", "ssh", "svn+ssh"];

const KNOWN_HOSTING_SITES: &[&str] = &[
    "code.launchpad.net",
    "github.com",
    "launchpad.net",
    "git.openstack.org",
];

pub fn plausible_url(url: &str) -> bool {
    url.contains(':')
}

pub fn drop_vcs_in_scheme(url: &Url) -> Option<Url> {
    let scheme = url.scheme();
    match scheme {
        "git+http" | "git+https" => {
            Some(derive_with_scheme(url, scheme.trim_start_matches("git+")))
        }
        "hg+http" | "hg+https" => {
            Some(derive_with_scheme(url, scheme.trim_start_matches("hg+")))
        }
        "bzr+lp" | "bzr+http" => {
            Some(derive_with_scheme(url, scheme.trim_start_matches("bzr+")))
        }
        _ => None,
    }
}

pub fn split_vcs_url(location: &str) -> (String, Option<String>, Option<String>) {
    let mut url = location.to_string();
    let mut branch = None;
    let mut subpath = None;
    if let Some(idx) = url.find('[') {
        if let Some(idx2) = url.find(']') {
            subpath = Some(url[idx + 1..idx2].to_string());
            url = url[..idx].to_string();
        }
    }
    if let Some(idx) = url.find(" -b ") {
        branch = Some(url[idx + 4..].to_string());
        url = url[..idx].to_string();
    }
    (url, branch, subpath)
}

pub fn unsplit_vcs_url(location: &VcsLocation) -> String {
    let mut url = location.url.to_string();
    if let Some(branch_name) = location.branch.as_deref() {
        url = format!("{} -b {}", url, branch_name);
    }
    if let Some(subpath_str) = location.subpath.as_deref() {
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
        tags.extract()
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
        probe_upstream_github_branch_url(url, version)
    } else {
        probe_upstream_breezy_branch_url(url, version)
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
                                .trim_end_matches('.'),
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
                                .trim_end_matches('.'),
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
        Ok(_data) => true,
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
            if path_segments.is_empty() {
                return None;
            }
            let proj_segments = if path_segments.contains(&"-") {
                path_segments[0..path_segments.iter().position(|s| s.contains('-')).unwrap()]
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
            } else if path_segments.is_empty() {
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

pub fn canonical_git_repo_url(repo_url: &Url, net_access: Option<bool>) -> Option<Url> {
    if let Some(hostname) = repo_url.host_str() {
        if (is_gitlab_site(hostname, net_access) || hostname == "github.com")
            && !repo_url.path().ends_with(".git")
        {
            let mut url = repo_url.clone();
            url.set_path(&(url.path().to_owned() + ".git"));
            return Some(url);
        }
    }
    None
}

pub fn browse_url_from_repo_url(
    location: &VcsLocation,
    net_access: Option<bool>,
) -> Option<url::Url> {
    if location.url.host_str() == Some("github.com") {
        let mut path = location.url
            .path_segments()
            .unwrap()
            .take(3)
            .collect::<Vec<&str>>()
            .join("/");
        if path.ends_with(".git") {
            path = path[..path.len() - 4].to_string();
        }
        if location.subpath.is_some() || location.branch.is_some() {
            path.push_str(&format!("/tree/{}", location.branch.as_deref().unwrap_or("HEAD")));
        }
        if let Some(subpath_str) = location.subpath.as_deref() {
            path.push_str(&format!("/{}", subpath_str));
        }
        Some(
            Url::parse("https://github.com")
                .unwrap()
                .join(&path)
                .unwrap()
        )
    } else if location.url.host_str() == Some("gopkg.in") {
        let mut els = location.url
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
            if let Some(subpath_str) = location.subpath.as_deref() {
                path.push_str(&format!("/{}", subpath_str));
            }
            Some(
                Url::parse("https://github.com")
                    .unwrap()
                    .join(&path)
                    .unwrap()
            )
        } else {
            None
        }
    } else if location.url.host_str() == Some("code.launchpad.net")
        || location.url.host_str() == Some("launchpad.net")
    {
        let mut path = location.url.path().to_string();
        if let Some(subpath_str) = location.subpath.as_deref() {
            path.push_str(&format!("/view/head:{}", subpath_str));
            return Some(
                Url::parse(format!("https://bazaar.launchpad.net{}", path).as_str())
                    .unwrap()
            );
        } else {
            return Some(
                Url::parse(format!("https://code.launchpad.net{}", path).as_str())
                    .unwrap()
            );
        }
    } else if location.url.host_str() == Some("svn.apache.org") {
        let path_elements = location.url
            .path_segments()
            .map(|segments| segments.into_iter().collect::<Vec<&str>>())
            .unwrap_or_else(Vec::new);
        if path_elements.len() >= 2 && path_elements[0] == "repos" && path_elements[1] == "asf"
        {
            let mut path_elements = path_elements.into_iter().skip(1).collect::<Vec<&str>>();
            path_elements[0] = "viewvc";
            if let Some(subpath_str) = location.subpath.as_deref() {
                path_elements.push(subpath_str);
            }
            return Some(
                Url::parse(
                    format!("https://svn.apache.org{}", path_elements.join("/")).as_str(),
                )
                .unwrap()
            );
        } else {
            None
        }
    } else if location.url.host_str() == Some("git.savannah.gnu.org")
        || location.url.host_str() == Some("git.sv.gnu.org")
    {
        let mut path_elements = location.url.path_segments().unwrap().collect::<Vec<&str>>();
        if location.url.scheme() == "https" && path_elements.first() == Some(&"git") {
            path_elements.remove(0);
        }
        path_elements.insert(0, "cgit");
        if let Some(subpath_str) = location.subpath.as_deref() {
            path_elements.push("tree");
            path_elements.push(subpath_str);
        }
        Some(
            Url::parse(
                format!("https://git.savannah.gnu.org{}", path_elements.join("/")).as_str(),
            )
            .unwrap()
        )
    } else if location.url.host_str().is_some() && is_gitlab_site(location.url.host_str().unwrap(), net_access) {
        let mut path = location.url.path().to_string();
        if path.ends_with(".git") {
            path = path[..path.len() - 4].to_string();
        }
        if let Some(subpath_str) = location.subpath.as_deref() {
            path.push_str(&format!("/-/blob/HEAD/{}", subpath_str));
        }
        Some(
            Url::parse(format!("https://{}{}", location.url.host_str().unwrap(), path).as_str())
                .unwrap()
        )
    } else {
        None
    }
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

pub fn fixup_rcp_style_git_repo_url(url: &str) -> Option<Url> {
    pyo3::prepare_freethreaded_python();
    breezyshim::location::rcp_location_to_url(url).ok()
}

pub fn try_open_branch(url: &url::Url, branch_name: Option<&str>) -> Option<Box<dyn breezyshim::branch::Branch>> {
    match Python::with_gil(|py| {
        let uim = py.import("breezy.ui")?;
        let controldirm = py.import("breezy.controldir")?;
        let controldir_cls = controldirm.getattr("ControlDir")?;

        let old_ui_factory = uim.getattr("ui_factory")?;
        uim.setattr("ui_factory", uim.call_method0("SilentUIFactory")?)?;

        let r = || -> PyResult<PyObject>{
            let c = controldir_cls.call_method1("open", (url.to_string(),))?;
            let b = c.call_method1("open_branch", (branch_name, ))?;

            b.call_method0("last_revision")?;
            Ok(b.to_object(py))
        }();

        uim.setattr("ui_factory", old_ui_factory)?;

        match r {
            Ok(b) => Ok(b),
            Err(e) => Err(e)
        }
    }) {
        Ok(b) => Python::with_gil(|py| Some(Box::new(breezyshim::branch::RegularBranch::new(b.to_object(py))) as Box<dyn breezyshim::branch::Branch>)),
        Err(_) => None
    }
}

pub fn find_secure_repo_url(
    mut url: url::Url, branch: Option<&str>, net_access: Option<bool>
) -> Option<url::Url> {
    if SECURE_SCHEMES.contains(&url.scheme()) {
        return Some(url);
    }

    // Sites we know to be available over https
    if let Some(hostname) = url.host_str() {
        if is_gitlab_site(hostname, net_access) || vec![ "github.com", "git.launchpad.net", "bazaar.launchpad.net", "code.launchpad.net", ].contains(&hostname) {
            url = derive_with_scheme(&url, "https");
        }
    }

    if url.scheme() == "lp" {
        url = derive_with_scheme(&url, "https");
        url.set_host(Some("code.launchpad.net")).unwrap();
    }

    if let Some(host) = url.host_str() {
        if vec!["git.savannah.gnu.org", "git.sv.gnu.org"].contains(&host) {
            if url.scheme() == "http" {
                url = derive_with_scheme(&url, "https");
            } else {
                url = derive_with_scheme(&url, "https");
                url.set_path(format!("/git{}", url.path()).as_str());
            }
        }
    } else {
        return None;
    }


    if net_access.unwrap_or(true) {
        let secure_repo_url = derive_with_scheme(&url, "https");
        let insecure_branch = try_open_branch(&url, branch);
        let secure_branch = try_open_branch(&secure_repo_url, branch);
        if let Some(secure_branch) = secure_branch {
            if insecure_branch.is_none() || secure_branch.last_revision() == insecure_branch.unwrap().last_revision() {
                url = secure_repo_url;
            }
        }
    }

    if SECURE_SCHEMES.contains(&url.scheme()) {
        Some(url)
    } else {
        // Can't find a secure URI :(
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcsLocation {
    pub url: url::Url,
    pub branch: Option<String>,
    pub subpath: Option<String>,
}

impl ToString for VcsLocation {
    fn to_string(&self) -> String {
        unsplit_vcs_url(self)
    }
}

impl From<VcsLocation> for url::Url {
    fn from(v: VcsLocation) -> Self {
        v.url
    }
}

impl From<url::Url> for VcsLocation {
    fn from(url: url::Url) -> Self {
        VcsLocation {
            url,
            branch: None,
            subpath: None,
        }
    }
}

impl From<&str> for VcsLocation {
    fn from(url: &str) -> Self {
        let (url, branch, subpath) = split_vcs_url(url);
        let url = fixup_git_url(url.as_str());
        VcsLocation {
            url: url.parse().unwrap(),
            branch,
            subpath
        }
    }
}

fn derive_with_scheme(url: &url::Url, scheme: &str) -> url::Url {
    let mut s = url.to_string();
    s.replace_range(..url.scheme().len(), scheme);
    url::Url::parse(&s).unwrap()
}

fn fix_path_in_port(url: &str) -> Option<String> {
    let (_, scheme, host, port, rest)  = match lazy_regex::regex_captures!(r"^([^:]+)://([^:]+):([^/]+)(/.*)$", url) {
        Some(c) => c,
        None => return None,
    };

    if port.ends_with(']') {
        return None;
    }

    if let Ok(port) = port.parse::<u16>() {
        Some(format!("{}://{}:{}{}", scheme, host, port, rest))
    } else {
        Some(format!("{}://{}/{}{}", scheme, host, port, rest))
    }
}

fn fix_gitlab_scheme(url: &str) -> Option<String> {
    if let Ok(url) = url::Url::parse(url) {
        if let Some(host) = url.host_str() {
            if is_gitlab_site(host, None) {
                return Some(derive_with_scheme(&url, "https").to_string());
            }
        }
    }
    None
}


fn fix_github_scheme(url: &str) -> Option<String> {
    // GitHub no longer supports the git:// scheme
    if let Ok(url) = url::Url::parse(url) {
        if url.host_str() == Some("github.com") {
            return Some(derive_with_scheme(&url, "https").to_string());
        }
    }
    None
}

fn fix_salsa_cgit_url(url: &str) -> Option<String> {
    if let Ok(url) = url::Url::parse(url) {
        if url.host_str() == Some("salsa.debian.org") {
            if let Some(suffix) = url.path().strip_prefix("/cgit/") {
                let mut url = url.clone();
                url.set_path(suffix);
                return Some(url.to_string());
            }
        }
    }
    None
}

fn fix_gitlab_tree_in_url(location: &VcsLocation) -> Option<VcsLocation> {
    if is_gitlab_site(location.url.host_str()?, None) {
        let segments = location.url.path_segments().unwrap().collect::<Vec<_>>();
        if let Some(p) = segments.iter().position(|p| *p == "tree") {
            let branch = segments[(p + 1)..].join("/");
            let path = segments[..p].join("/");
            let mut url = location.url.clone();
            url.set_path(path.as_str());
            return Some(VcsLocation {
                url,
                branch: Some(branch),
                subpath: location.subpath.clone(),
            });
        }
    }
    None
}


fn fix_double_slash(url: &str) -> Option<String> {
    if let Ok(mut url) = url::Url::parse(url) {
        if url.path().starts_with("//") {
            let path = url.path().to_string().strip_prefix("//").unwrap().to_string();
            url.set_path(path.as_str());
            return Some(url.to_string());
        }
    }
    None
}

fn fix_extra_colon(url: &str) -> Option<String> {
    if let Ok(mut url) = url::Url::parse(url) {
        if url.path().starts_with(':') {
            let path = url.path().to_string().strip_prefix(':').unwrap().to_string();
            url.set_path(&path);
            return Some(url.to_string());
        }
    }
    None
}


fn drop_git_username(url: &str) -> Option<String> {
    if let Ok(mut url) = url::Url::parse(url) {
        if let Some(host) = url.host_str() {
            if !["github.com", "salsa.debian.org", "gitlab.com"].contains(&host) {
                return None;
            }
        } else {
            return None;
        }
        if !["git", "http", "https"].contains(&url.scheme()) {
            return None;
        }
        if url.username() == "git" {
            url.set_username("").unwrap();
            return Some( url.to_string());
        }
    }
    None
}


fn fix_branch_argument(location: &VcsLocation) -> Option<VcsLocation> {
    if location.url.host_str() == Some("github.com") {
        // TODO(jelmer): Handle gitlab sites too?
        let path_elements = location.url.path_segments().unwrap().collect::<Vec<_>>();
        if path_elements.len() > 2 && path_elements[2] == "tree" {
            let branch = path_elements[3..].join("/");
            let path = path_elements[..2].join("/");
            let mut url = location.url.clone();
            url.set_path(path.as_str());
            Some(VcsLocation {
                url,
                branch: Some(branch),
                subpath: location.subpath.clone(),
            })
        } else {
            None
        }
    } else {
        None
    }
}


fn fix_git_gnome_org_url(url: &str) -> Option<String> {
    if let Ok(url) = url::Url::parse(url) {
        if url.host_str() == Some("git.gnome.org") {
            let mut path_segments = url.path_segments().unwrap().collect::<Vec<_>>();
            if path_segments.first().map(|p| *p == "browse") == Some(true) {
                path_segments.remove(0);
            }

            let mut url = derive_with_scheme(&url, "https");
            url.set_host(Some("gitlab.gnome.org")).unwrap();
            url.set_path(format!("GNOME/{}", path_segments.join("/")).as_str());
            return Some(url.to_string());
        }
    }
    None
}


fn fix_kde_anongit_url(url: &str) -> Option<String> {
    if let Ok(url) = url::Url::parse(url) {
        if url.host_str() == Some("anongit.kde.org") {
            let url = derive_with_scheme(&url, "https");
            return Some(url.to_string());
        }
    }
    None
}

fn fix_freedesktop_org_url(url: &str) -> Option<String> {
    if let Ok(url) = url::Url::parse(url) {
        if url.host_str() == Some("anongit.freedesktop.org") {
            let suffix = url.path().strip_prefix("/git/");
            let mut url = derive_with_scheme(&url, "https");
            if let Some(suffix) = suffix {
                url.set_path(suffix);
            }
            url.set_host(Some("gitlab.freedesktop.org")).unwrap();
            return Some(url.to_string());
        }
    }
    None
}

const LOCATION_FIXERS: &[fn(&VcsLocation) -> Option<VcsLocation>] = &[
    fix_gitlab_tree_in_url,
    fix_branch_argument,
];

/// Attempt to fix up broken Git URLs.
pub fn fixup_git_location(
    location: &VcsLocation,
) -> Cow<'_, VcsLocation> {
    let mut location = Cow::Borrowed(location);
    for cb in LOCATION_FIXERS {
        location =  cb(&location).map_or(location, Cow::Owned);
    }
    location
}

const URL_FIXERS: &[fn(&str) -> Option<String>] = &[
    fix_path_in_port,
    fix_gitlab_scheme,
    fix_github_scheme,
    fix_salsa_cgit_url,
    fix_double_slash,
    fix_extra_colon,
    drop_git_username,
    fix_freedesktop_org_url,
    fix_kde_anongit_url,
    fix_git_gnome_org_url,
];

pub fn fixup_git_url(url: &str) -> String {
    let mut url = url.to_string();
    for cb in URL_FIXERS {
        url = cb(&url).unwrap_or(url);
    }
    url
}

pub fn convert_cvs_list_to_str(urls: &[&str]) -> Option<String> {
    if urls[0].starts_with(":extssh:") || urls[0].starts_with(":pserver:") {
        let url = breezyshim::location::cvs_to_url(urls[0]);
        Some(format!("{}#{}", url, urls[1]))
    } else {
        None
    }
}


pub const SANITIZERS: &[fn(&str) -> Option<Url>] = &[
    |url| drop_vcs_in_scheme(&url.parse().unwrap()),
    |url| Some(fixup_git_location(&VcsLocation::from(url)).url.clone()),
    fixup_rcp_style_git_repo_url,
    |url| find_public_repo_url(url.to_string().as_str(), None).map(|u| u.parse().unwrap()),
    |url| canonical_git_repo_url(&url.parse().unwrap(), None),
    |url| find_secure_repo_url(url.parse().unwrap(), None, Some(false)),
];

pub fn sanitize_url(url: &Url)-> Url {
    let mut url: Cow<'_, Url> = Cow::Borrowed(url);
    for sanitizer in SANITIZERS {
        url = sanitizer(url.to_string().as_str()).map_or(url, Cow::Owned);
    }
    url.into_owned()
}

#[cfg(test)]
mod tests {
    use super::fixup_git_url;

    fn fixup_git_location(url: &str) -> String {
        super::fixup_git_location(&super::VcsLocation::from(url)).to_string()
    }

    #[test]
    fn test_plausible_url() {
        use super::plausible_url;
        assert!(!plausible_url("the"));
        assert!(!plausible_url("1"));
        assert!(plausible_url("git@foo:blah"));
        assert!(plausible_url("git+ssh://git@foo/blah"));
        assert!(plausible_url("https://foo/blah"));
    }


    #[test]
    fn test_is_gitlab_site() {
        use super::is_gitlab_site;

        assert!(is_gitlab_site("gitlab.com", Some(false)));
        assert!(is_gitlab_site("gitlab.example.com", Some(false)));
        assert!(is_gitlab_site("salsa.debian.org", Some(false)));
        assert!(!is_gitlab_site("github.com", Some(false)));
        assert!(!is_gitlab_site("foo.example.com", Some(false)));
    }

    #[test]
    pub fn test_canonicalize_github() {
        use super::canonical_git_repo_url;
        use url::Url;
        assert_eq!(
            Some("https://github.com/jelmer/example.git".parse::<Url>().unwrap()),
            canonical_git_repo_url(&"https://github.com/jelmer/example".parse::<Url>().unwrap(), Some(false))
        );
    }

    #[test]
    pub fn test_canonicalize_github_ssh() {
        use super::canonical_git_repo_url;
        use url::Url;
        assert_eq!(
            Some("https://salsa.debian.org/jelmer/example.git".parse::<Url>().unwrap()),
            canonical_git_repo_url(&"https://salsa.debian.org/jelmer/example".parse::<Url>().unwrap(), Some(false))
        );
        assert_eq!(
            None,
            canonical_git_repo_url(&"https://salsa.debian.org/jelmer/example.git".parse::<Url>().unwrap(), Some(false))
        );
    }

    #[test]
    fn test_find_public_github() {
        use super::find_public_repo_url;
        assert_eq!(
            "https://github.com/jelmer/example",
            find_public_repo_url("ssh://git@github.com/jelmer/example", Some(false)).unwrap()
        );
        assert_eq!(
            Some("https://github.com/jelmer/example"),
            find_public_repo_url("https://github.com/jelmer/example", Some(false)).as_deref()
        );
        assert_eq!(
            "https://github.com/jelmer/example",
            find_public_repo_url("git@github.com:jelmer/example", Some(false)).unwrap().as_str()
        );
    }

    #[test]
    fn test_find_public_salsa() {
        use super::find_public_repo_url;
        assert_eq!(
            "https://salsa.debian.org/jelmer/example",
            find_public_repo_url("ssh://salsa.debian.org/jelmer/example", Some(false)).unwrap().as_str()
        );
        assert_eq!(
            "https://salsa.debian.org/jelmer/example",
            find_public_repo_url("https://salsa.debian.org/jelmer/example", Some(false)).unwrap().as_str()
        );
    }
    #[test]
    fn test_fixup_rcp_style() {
        use super::fixup_rcp_style_git_repo_url;
        use url::Url;
        assert_eq!(
            Some("ssh://git@github.com/jelmer/example".parse::<Url>().unwrap()),
            fixup_rcp_style_git_repo_url("git@github.com:jelmer/example")
        );

        assert_eq!(
            Some("ssh://github.com/jelmer/example".parse::<Url>().unwrap()),
            fixup_rcp_style_git_repo_url("github.com:jelmer/example")
        );
    }

    #[test]
    fn test_fixup_rcp_leave() {
        use super::fixup_rcp_style_git_repo_url;
        assert_eq!(
            None,
            fixup_rcp_style_git_repo_url("https://salsa.debian.org/jelmer/example")
        );
        assert_eq!(
            None,
            fixup_rcp_style_git_repo_url("ssh://git@salsa.debian.org/jelmer/example")
        );
    }

    #[test]
    fn test_guess_repo_url_travis_ci_org() {
        use super::guess_repo_from_url;
        assert_eq!(
            Some("https://github.com/jelmer/dulwich"),
            guess_repo_from_url(&"https://travis-ci.org/jelmer/dulwich".parse().unwrap(), Some(false)).as_deref(),
        );
    }

    #[test]
    fn test_guess_repo_url_coveralls() {
        use super::guess_repo_from_url;
        assert_eq!(
            Some("https://github.com/jelmer/dulwich"),
            guess_repo_from_url(&"https://coveralls.io/r/jelmer/dulwich".parse().unwrap(), Some(false)).as_deref(),
        );
    }

    #[test]
    fn test_guess_repo_url_gitlab() {
        use super::guess_repo_from_url;
        assert_eq!(
            Some("https://gitlab.com/jelmer/dulwich"),
            guess_repo_from_url(&"https://gitlab.com/jelmer/dulwich".parse().unwrap(), Some(false)).as_deref(),
        );
        assert_eq!(
            Some("https://gitlab.com/jelmer/dulwich"),
            guess_repo_from_url(&"https://gitlab.com/jelmer/dulwich/tags".parse().unwrap(), Some(false)).as_deref(),
        );
    }

    #[test]
    fn test_fixup_git_location() {
        use super::{fixup_git_location, VcsLocation};
        assert_eq!(
            VcsLocation {
                url: "https://github.com/jelmer/dulwich".parse().unwrap(),
                branch: None,
                subpath: None,
            },
            fixup_git_location(&VcsLocation {
                url: "https://github.com/jelmer/dulwich".parse().unwrap(),
                branch: None,
                subpath: None,
            }).into_owned()
        );
    }

    #[test]
    fn test_browse_url_from_repo() {
        use super::browse_url_from_repo_url;
        assert_eq!(
            Some("https://github.com/jelmer/dulwich".parse().unwrap()),
            browse_url_from_repo_url(&super::VcsLocation {
                url: "https://github.com/jelmer/dulwich".parse().unwrap(),
                branch: None,
                subpath: None,
            },
            Some(false)),
        );
        assert_eq!(
            Some("https://github.com/jelmer/dulwich".parse().unwrap()),
            browse_url_from_repo_url(&super::VcsLocation {
                url: "https://github.com/jelmer/dulwich.git".parse().unwrap(),
                branch: None,
                subpath: None,
            }, Some(false))
        );
        assert_eq!(
            Some("https://github.com/jelmer/dulwich/tree/foo".parse().unwrap()),
            browse_url_from_repo_url(&super::VcsLocation {
                url: "https://github.com/jelmer/dulwich.git".parse().unwrap(),
                branch: Some("foo".to_string()),
                subpath: None,
            }, Some(false))
        );
        assert_eq!(
            Some("https://github.com/jelmer/dulwich/tree/HEAD/foo".parse().unwrap()),
            browse_url_from_repo_url(
                &super::VcsLocation {
                    url: "https://github.com/jelmer/dulwich.git".parse().unwrap(),
                    branch: None,
                    subpath: Some("foo".to_string()),
                }, Some(false)
            )
        );
    }

    #[test]
    fn test_fix_github_scheme() {
        use super::fix_github_scheme;
        assert_eq!(
            Some("https://github.com/jelmer/example"),
            fix_github_scheme("git://github.com/jelmer/example").as_deref()
        );
    }

    #[test]
    fn test_fix_git_gnome_org_url() {
        use super::fix_git_gnome_org_url;
        assert_eq!(
            Some("https://gitlab.gnome.org/GNOME/example".to_string()),
            fix_git_gnome_org_url("https://git.gnome.org/browse/example")
        );
    }

    #[test]
    fn test_fixup() {
        assert_eq!(
            "https://github.com/jelmer/dulwich",
            fixup_git_url("https://github.com:jelmer/dulwich")
        );
        assert_eq!(
            "https://github.com/jelmer/dulwich -b blah",
            fixup_git_location("https://github.com:jelmer/dulwich -b blah"),
        );
        assert_eq!(
            "https://github.com/jelmer/dulwich",
            fixup_git_url("git://github.com/jelmer/dulwich"),
        );
    }

    #[test]
    fn test_preserves() {
        assert_eq!(
            "https://github.com/jelmer/dulwich",
            fixup_git_url("https://github.com/jelmer/dulwich"),
        );
    }

    #[test]
    fn test_salsa_not_https() {
        assert_eq!(
            "https://salsa.debian.org/jelmer/dulwich",
            fixup_git_url("git://salsa.debian.org/jelmer/dulwich"),
        );
    }

    #[test]
    fn test_salsa_uses_cgit() {
        assert_eq!(
            "https://salsa.debian.org/jelmer/dulwich",
            fixup_git_url(
                "https://salsa.debian.org/cgit/jelmer/dulwich"
            ),
        );
    }

    #[test]
    fn test_salsa_tree_branch() {
        assert_eq!(
            "https://salsa.debian.org/jelmer/dulwich -b master",
            fixup_git_location(
                "https://salsa.debian.org/jelmer/dulwich/tree/master"
            ),
        );
    }

    #[test]
    fn test_strip_extra_slash() {
        assert_eq!(
            "https://salsa.debian.org/salve/auctex.git",
            fixup_git_url("https://salsa.debian.org//salve/auctex.git"),
        );
    }

    #[test]
    fn test_strip_extra_colon() {
        assert_eq!(
            "https://salsa.debian.org/mckinstry/lcov.git",
            fixup_git_url(
                "https://salsa.debian.org:/mckinstry/lcov.git"
            ),
        );
    }

    #[test]
    fn test_strip_username() {
        assert_eq!(
            "https://github.com/RPi-Distro/pgzero.git",
            fixup_git_url("git://git@github.com:RPi-Distro/pgzero.git"),
        );
        assert_eq!(
            "https://salsa.debian.org/debian-astro-team/pyavm.git",
            fixup_git_url(
                "https://git@salsa.debian.org:debian-astro-team/pyavm.git"
            ),
        );
    }

    #[test]
    fn test_github_tree_url() {
        assert_eq!(
            "https://github.com/blah/blah -b master",
            fixup_git_location("https://github.com/blah/blah/tree/master"),
        );
    }

    #[test]
    fn test_freedesktop() {
        assert_eq!(
            "https://gitlab.freedesktop.org/xorg/xserver",
            fixup_git_url("git://anongit.freedesktop.org/xorg/xserver"),
        );
        assert_eq!(
            "https://gitlab.freedesktop.org/xorg/lib/libSM",
            fixup_git_url(
                "git://anongit.freedesktop.org/git/xorg/lib/libSM"
            ),
        );
    }

    #[test]
    fn test_anongit() {
        assert_eq!(
            "https://anongit.kde.org/kdev-php.git",
            fixup_git_url("git://anongit.kde.org/kdev-php.git"),
        );
    }

    #[test]
    fn test_gnome() {
        assert_eq!(
            "https://gitlab.gnome.org/GNOME/alacarte",
            fixup_git_url("https://git.gnome.org/browse/alacarte"),
        );
    }
}
