pub mod arch;
pub mod authors;
pub mod autoconf;
pub mod composer_json;
pub mod debian;
pub mod doap;
pub mod git;
pub mod go;
pub mod gobo;
pub mod haskell;
pub mod launchpad;
pub mod maven;
pub mod meson;
pub mod metadata_json;
pub mod metainfo;
pub mod nuspec;
#[cfg(feature = "opam")]
pub mod ocaml;
pub mod package_json;
pub mod package_xml;
pub mod package_yaml;
pub mod perl;
pub mod php;
pub mod pubspec;
pub mod python;
pub mod r;
pub mod repology;
pub mod ruby;
#[cfg(feature = "cargo")]
pub mod rust;
pub mod security_md;
pub mod waf;

use crate::{Certainty, GuesserSettings, UpstreamDatum, UpstreamDatumWithMetadata};
use std::io::BufRead;

pub fn guess_from_install(
    path: &std::path::Path,
    _settings: &GuesserSettings,
) -> Result<Vec<crate::UpstreamDatumWithMetadata>, crate::ProviderError> {
    let mut ret = Vec::new();

    let f = std::fs::File::open(path)?;

    let f = std::io::BufReader::new(f);

    let mut urls: Vec<String> = Vec::new();
    let mut lines = f.lines();
    while let Some(oline) = lines.next() {
        let oline = oline?;
        let line = oline.trim();
        let mut cmdline = line.trim().trim_start_matches('$').trim().to_string();
        if cmdline.starts_with("git clone ") || cmdline.starts_with("fossil clone ") {
            while cmdline.ends_with('\\') {
                cmdline.push_str(lines.next().unwrap()?.trim());
                cmdline = cmdline.trim().to_string();
            }
            if let Some(url) = if cmdline.starts_with("git clone ") {
                crate::vcs_command::url_from_git_clone_command(cmdline.as_bytes())
            } else if cmdline.starts_with("fossil clone ") {
                crate::vcs_command::url_from_fossil_clone_command(cmdline.as_bytes())
            } else {
                None
            } {
                urls.push(url);
            }
        }
        for m in lazy_regex::regex!("[\"'`](git clone.*)[\"`']").find_iter(line) {
            if let Some(url) = crate::vcs_command::url_from_git_clone_command(m.as_str().as_bytes())
            {
                urls.push(url);
            }
        }
        let project_re = "([^/]+)/([^/?.()\"#>\\s]*[^-/?.()\"#>\\s])";
        for m in regex::Regex::new(format!("https://github.com/{}/(.git)?", project_re).as_str())
            .unwrap()
            .find_iter(line)
        {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(m.as_str().trim_end_matches('.').to_string()),
                certainty: Some(Certainty::Possible),
                origin: Some(path.into()),
            });
        }

        if let Some(m) = regex::Regex::new(format!("https://github.com/{}", project_re).as_str())
            .unwrap()
            .captures(line)
        {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(
                    m.get(0).unwrap().as_str().trim_end_matches('.').to_string(),
                ),
                certainty: Some(Certainty::Possible),
                origin: Some(path.into()),
            });
        }
        if let Some((url, _)) = lazy_regex::regex_captures!("git://([^ ]+)", line) {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(url.trim_end_matches('.').to_string()),
                certainty: Some(Certainty::Possible),
                origin: Some(path.into()),
            });
        }
        for m in lazy_regex::regex!("https://([^]/]+)/([^]\\s()\"#]+)").find_iter(line) {
            let url: url::Url = m.as_str().trim_end_matches('.').trim().parse().unwrap();
            if crate::vcs::is_gitlab_site(url.host_str().unwrap(), None) {
                if let Some(repo_url) = crate::vcs::guess_repo_from_url(&url, None) {
                    ret.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Repository(repo_url),
                        certainty: Some(Certainty::Possible),
                        origin: Some(path.into()),
                    });
                }
            }
        }
    }
    Ok(ret)
}
