/// Arch Linux package metadata provider
pub mod arch;
/// Authors file metadata provider
pub mod authors;
/// Autoconf configure script metadata provider
pub mod autoconf;
/// PHP Composer metadata provider
pub mod composer_json;
/// Debian package metadata provider
pub mod debian;
/// DOAP (Description of a Project) metadata provider
pub mod doap;
/// Git configuration metadata provider
pub mod git;
/// Go module metadata provider
pub mod go;
/// GoboLinux metadata provider
pub mod gobo;
/// Haskell package metadata provider
pub mod haskell;
/// Launchpad metadata provider
pub mod launchpad;
/// Maven POM metadata provider
pub mod maven;
/// Meson build system metadata provider
pub mod meson;
/// Generic metadata.json provider
pub mod metadata_json;
/// AppStream metainfo metadata provider
pub mod metainfo;
/// Node.js metadata provider
pub mod node;
/// NuGet package specification metadata provider
pub mod nuspec;
/// OCaml OPAM metadata provider
#[cfg(feature = "opam")]
pub mod ocaml;
/// NPM package.json metadata provider
pub mod package_json;
/// PEAR package.xml metadata provider
pub mod package_xml;
/// Haskell package.yaml metadata provider
pub mod package_yaml;
/// Perl module metadata provider
pub mod perl;
/// PHP package metadata provider
pub mod php;
/// Dart/Flutter pubspec metadata provider
pub mod pubspec;
/// Python package metadata provider
pub mod python;
/// R package metadata provider
pub mod r;
/// Repology metadata provider
pub mod repology;
/// Ruby gem metadata provider
pub mod ruby;
/// Rust crate metadata provider
pub mod rust;
/// Security.md file metadata provider
pub mod security_md;
/// Waf build system metadata provider
pub mod waf;

use crate::{Certainty, GuesserSettings, UpstreamDatum, UpstreamDatumWithMetadata};
use std::io::BufRead;

/// Guesses upstream metadata from INSTALL file
pub async fn guess_from_install(
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
            if crate::vcs::is_gitlab_site(url.host_str().unwrap(), None).await {
                if let Some(repo_url) = crate::vcs::guess_repo_from_url(&url, None).await {
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
