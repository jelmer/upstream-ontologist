use crate::{
    bug_database_from_issue_url, repo_url_from_merge_request_url, Certainty, GuesserSettings,
    Origin, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata,
};
use lazy_regex::regex_captures;
use log::debug;
use std::fs::File;
use std::io::BufRead;
use std::io::Read;
use std::path::Path;
use url::Url;

pub fn guess_from_debian_patch(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let file = File::open(path)?;
    let reader = std::io::BufReader::new(file);

    let net_access = None;

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();

    for line in reader.lines().map_while(Result::ok) {
        if line.starts_with("Forwarded: ") {
            let forwarded = match line.split_once(':') {
                Some((_, url)) => url.trim(),
                None => {
                    debug!("Malformed Forwarded line in patch {}", path.display());
                    continue;
                }
            };
            let forwarded = match Url::parse(forwarded) {
                Ok(url) => url,
                Err(e) => {
                    debug!(
                        "Malformed URL in Forwarded line in patch {}: {}",
                        path.display(),
                        e
                    );
                    continue;
                }
            };

            if let Some(bug_db) = bug_database_from_issue_url(&forwarded, net_access) {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::BugDatabase(bug_db.to_string()),
                    certainty: Some(Certainty::Possible),
                    origin: Some(path.into()),
                });
            }

            if let Some(repo_url) = repo_url_from_merge_request_url(&forwarded, net_access) {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(repo_url.to_string()),
                    certainty: Some(Certainty::Possible),
                    origin: Some(path.into()),
                });
            }
        }
    }

    Ok(upstream_data)
}

pub fn metadata_from_itp_bug_body(
    body: &str,
    origin: Option<Origin>,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut results: Vec<UpstreamDatumWithMetadata> = Vec::new();
    // Skip first few lines with bug metadata (severity, owner, etc)
    let mut line_iter = body.split_terminator('\n');
    let mut next_line = line_iter.next();

    while let Some(line) = next_line {
        if next_line.is_none() {
            return Err(ProviderError::ParseError(
                "ITP bug body ended before package name".to_string(),
            ));
        }
        next_line = line_iter.next();
        if line.trim().is_empty() {
            break;
        }
    }

    while let Some(line) = next_line {
        if next_line.is_none() {
            return Err(ProviderError::ParseError(
                "ITP bug body ended before package name".to_string(),
            ));
        }
        if !line.is_empty() {
            break;
        }
        next_line = line_iter.next();
    }

    while let Some(mut line) = next_line {
        line = line.trim_start_matches('*').trim_start();

        if line.is_empty() {
            break;
        }

        match line.split_once(':') {
            Some((key, value)) => {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "Package name" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Name(value.to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: origin.clone(),
                        });
                    }
                    "Version" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Version(value.to_string()),
                            certainty: Some(Certainty::Possible),
                            origin: origin.clone(),
                        });
                    }
                    "Upstream Author" if !value.is_empty() => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Author(vec![Person::from(value)]),
                            certainty: Some(Certainty::Confident),
                            origin: origin.clone(),
                        });
                    }
                    "URL" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Homepage(value.to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: origin.clone(),
                        });
                    }
                    "License" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::License(value.to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: origin.clone(),
                        });
                    }
                    "Description" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Summary(value.to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: origin.clone(),
                        });
                    }
                    _ => {
                        debug!("Unknown pseudo-header {} in ITP bug body", key);
                    }
                }
            }
            _ => {
                debug!("Ignoring non-semi-field line {}", line);
            }
        }

        next_line = line_iter.next();
    }

    let mut rest: Vec<String> = Vec::new();
    for line in line_iter {
        if line.trim() == "-- System Information:" {
            break;
        }
        rest.push(line.to_string());
    }

    results.push(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Description(rest.join("\n")),
        certainty: Some(Certainty::Likely),
        origin: origin.clone(),
    });

    Ok(results)
}

#[test]
fn test_metadata_from_itp_bug_body() {
    assert_eq!(
        vec![
            UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name("setuptools-gettext".to_string()),
                certainty: Some(Certainty::Confident),
                origin: None,
            },
            UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Version("0.0.1".to_string()),
                certainty: Some(Certainty::Possible),
                origin: None,
            },
            UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Author(vec![Person::from("Breezy Team <breezy-core@googlegroups.com>")]),
                certainty: Some(Certainty::Confident),
                origin: None,
            },
            UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Homepage("https://github.com/jelmer/setuptools-gettext".to_string()),
                certainty: Some(Certainty::Confident),
                origin: None,
            },
            UpstreamDatumWithMetadata {
                datum: UpstreamDatum::License("GPL".to_string()),
                certainty: Some(Certainty::Confident),
                origin: None,
            },
            UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Summary("Compile .po files into .mo files".to_string()),
                certainty: Some(Certainty::Confident),
                origin: None,
            },
            UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Description("This extension for setuptools compiles gettext .po files\nfound in the source directory into .mo files and installs them.\n".to_string()),
                certainty: Some(Certainty::Likely),
                origin: None,
            },
        ],
        metadata_from_itp_bug_body(
            r#"Package: wnpp
Severity: wishlist
Owner: Jelmer Vernooij <jelmer@debian.org>
Debbugs-Cc: debian-devel@lists.debian.org

* Package name    : setuptools-gettext
  Version         : 0.0.1
  Upstream Author : Breezy Team <breezy-core@googlegroups.com>
* URL             : https://github.com/jelmer/setuptools-gettext
* License         : GPL
  Programming Lang: Python
  Description     : Compile .po files into .mo files

This extension for setuptools compiles gettext .po files
found in the source directory into .mo files and installs them.

"#, None
        )
        .unwrap()
    );
}

pub fn guess_from_debian_changelog(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let cl = debian_changelog::ChangeLog::read_path(path).map_err(|e| {
        ProviderError::ParseError(format!(
            "Failed to parse changelog {}: {}",
            path.display(),
            e
        ))
    })?;

    let first_entry = cl
        .entries()
        .next()
        .ok_or_else(|| ProviderError::ParseError("Empty changelog".to_string()))?;

    let package = first_entry.package().ok_or_else(|| {
        ProviderError::ParseError(format!("Changelog {} has no package name", path.display()))
    })?;

    let mut ret = Vec::new();
    ret.push(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name(package.clone()),
        certainty: Some(Certainty::Confident),
        origin: Some(path.into()),
    });

    if let Some(version) = first_entry.version() {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Version(version.upstream_version),
            certainty: Some(Certainty::Confident),
            origin: Some(path.into()),
        });
    }

    #[cfg(feature = "debcargo")]
    if package.starts_with("rust-") {
        let debcargo_toml_path = path.parent().unwrap().join("debcargo.toml");
        let debcargo_config = debcargo::config::Config::parse(debcargo_toml_path.as_path())
            .map_err(|e| {
                ProviderError::ParseError(format!(
                    "Failed to parse debcargo config {}: {}",
                    path.display(),
                    e
                ))
            })?;

        let semver_suffix = debcargo_config.semver_suffix;
        let (mut crate_name, _crate_semver_version) =
            parse_debcargo_source_name(&package, semver_suffix);

        if crate_name.contains('-') {
            crate_name = match crate::providers::rust::cargo_translate_dashes(crate_name.as_str())
                .map_err(|e| {
                ProviderError::Other(format!(
                    "Failed to translate dashes in crate name {}: {}",
                    crate_name, e
                ))
            })? {
                Some(name) => name,
                None => {
                    return Err(ProviderError::Other(format!(
                        "Failed to translate dashes in crate name {}",
                        crate_name
                    )))
                }
            };
        }
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Archive("crates.io".to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });

        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::CargoCrate(crate_name),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(itp) = find_itp(first_entry.change_lines().collect::<Vec<_>>().as_slice()) {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::DebianITP(itp),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });

        ret.extend(guess_from_itp_bug(itp)?);
    }

    Ok(ret)
}

pub fn find_itp(changes: &[String]) -> Option<i32> {
    for line in changes {
        if let Some((_, itp)) = regex_captures!(r"\* Initial release. \(?Closes: #(\d+)\)?", line) {
            return Some(itp.parse().unwrap());
        }
    }
    None
}

pub fn guess_from_itp_bug(
    bugno: i32,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let debbugs = debbugs::blocking::Debbugs::default();

    let log = debbugs.get_bug_log(bugno).map_err(|e| {
        ProviderError::ParseError(format!("Failed to get bug log for bug {}: {}", bugno, e))
    })?;

    metadata_from_itp_bug_body(
        log[0].body.as_str(),
        Some(Origin::Other(format!("Debian bug #{}", bugno))),
    )
}

/// Parse a debcargo source name and return crate.
///
/// # Arguments
/// * `source_name` - Source package name
/// * `semver_suffix` - Whether semver_suffix is enabled
///
/// # Returns
/// tuple with crate name and optional semver
pub fn parse_debcargo_source_name(
    source_name: &str,
    semver_suffix: bool,
) -> (String, Option<String>) {
    let mut crate_name = source_name.strip_prefix("rust-").unwrap();
    match crate_name.rsplitn(2, '-').collect::<Vec<&str>>().as_slice() {
        [semver, new_crate_name] if semver_suffix => {
            crate_name = new_crate_name;
            (crate_name.to_string(), Some(semver.to_string()))
        }
        _ => (crate_name.to_string(), None),
    }
}

pub fn guess_from_debian_rules(
    path: &Path,
    _settings: &GuesserSettings,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let f = std::fs::File::open(path)?;
    let mf = makefile_lossless::Makefile::read_relaxed(f)
        .map_err(|e| ProviderError::ParseError(format!("Failed to parse debian/rules: {}", e)))?;

    let mut ret = vec![];

    if let Some(variable) = mf
        .variable_definitions()
        .find(|v| v.name().as_deref() == Some("DEB_UPSTREAM_GIT"))
    {
        let certainty = Some(Certainty::Likely);
        let datum = UpstreamDatum::Repository(variable.raw_value().unwrap());
        ret.push(UpstreamDatumWithMetadata {
            datum,
            certainty,
            origin: Some(Origin::Path(path.to_path_buf())),
        });
    }

    if let Some(deb_upstream_url) = mf
        .variable_definitions()
        .find(|v| v.name().as_deref() == Some("DEB_UPSTREAM_URL"))
    {
        let certainty = Some(Certainty::Likely);
        let datum = UpstreamDatum::Download(deb_upstream_url.raw_value().unwrap());
        ret.push(UpstreamDatumWithMetadata {
            datum,
            certainty,
            origin: Some(Origin::Path(path.to_path_buf())),
        });
    }

    Ok(ret)
}

pub fn guess_from_debian_control(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut ret = vec![];
    use std::str::FromStr;

    let control = debian_control::Control::from_str(&std::fs::read_to_string(path)?)
        .map_err(|e| ProviderError::ParseError(format!("Failed to parse debian/control: {}", e)))?;

    let source = control.source().unwrap();

    let is_native = debian_is_native(path.parent().unwrap()).map_err(|e| {
        ProviderError::ParseError(format!("Failed to parse debian/source/format: {}", e))
    })?;

    if let Some(homepage) = source.homepage() {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Homepage(homepage.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(go_import_path) = source.as_deb822().get("XS-Go-Import-Path") {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::GoImportPath(go_import_path.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });

        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository(format!("https://{}", go_import_path)),
            certainty: Some(Certainty::Likely),
            origin: Some(path.into()),
        });
    }

    if is_native == Some(true) {
        if let Some(vcs_git) = source.vcs_git() {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(vcs_git),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }

        if let Some(vcs_browser) = source.vcs_browser() {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::RepositoryBrowse(vcs_browser),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }
    }

    let binaries = control.binaries().collect::<Vec<_>>();

    let certainty = if binaries.len() == 1 && is_native == Some(true) {
        // Debian native package with only one binary package
        Certainty::Certain
    } else if binaries.len() > 1 && is_native == Some(true) {
        Certainty::Possible
    } else if binaries.len() == 1 && is_native == Some(false) {
        // Debian non-native package with only one binary package, so description is likely to be
        // good but might be Debian-specific
        Certainty::Confident
    } else {
        Certainty::Likely
    };

    for binary in binaries {
        if let Some(description) = binary.description() {
            let lines = description.split('\n').collect::<Vec<_>>();
            let mut summary = lines[0].to_string();
            let mut description_lines = &lines[1..];

            if !description_lines.is_empty()
                && description_lines
                    .last()
                    .unwrap()
                    .starts_with("This package contains")
            {
                summary = summary
                    .split(" - ")
                    .next()
                    .unwrap_or(summary.as_str())
                    .to_string();
                description_lines = description_lines.split_last().unwrap().1;
            }

            if !summary.is_empty() {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(summary),
                    certainty: Some(certainty),
                    origin: Some(path.into()),
                });
            }

            if !description_lines.is_empty() {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Description(description_lines.join("\n")),
                    certainty: Some(certainty),
                    origin: Some(path.into()),
                });
            }
        }
    }

    Ok(ret)
}

pub fn guess_from_debian_copyright(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut ret = vec![];
    let text = &std::fs::read_to_string(path)?;
    let mut urls = vec![];
    match debian_copyright::Copyright::from_str_relaxed(text) {
        Ok((c, _)) => {
            let header = c.header().unwrap();
            if let Some(upstream_name) = header.upstream_name() {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name(upstream_name.to_string()),
                    certainty: Some(if upstream_name.contains(' ') {
                        Certainty::Confident
                    } else {
                        Certainty::Certain
                    }),
                    origin: Some(path.into()),
                });
            }

            if let Some(upstream_contact) = header.upstream_contact() {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Contact(upstream_contact),
                    certainty: Some(Certainty::Possible),
                    origin: Some(path.into()),
                });
            }

            if let Some(source) = header.source() {
                if source.contains(' ') {
                    urls.extend(
                        source
                            .split(|c| c == ' ' || c == '\n' || c == ',')
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string()),
                    );
                } else {
                    urls.push(source.clone());
                }

                for (m, _, _) in
                    lazy_regex::regex_captures!(r"(http|https)://([^ ,]+)", source.as_str())
                {
                    urls.push(m.to_string());
                }
            }

            if let Some(upstream_bugs) = header.get("X-Upstream-Bugs") {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::BugDatabase(upstream_bugs),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }

            if let Some(source_downloaded_from) = header.get("X-Source-Downloaded-From") {
                if let Ok(url) = source_downloaded_from.parse::<url::Url>() {
                    urls.push(url.to_string());
                }

                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Download(source_downloaded_from),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }

            let referenced_licenses = c
                .iter_licenses()
                .filter_map(|l| l.name())
                .collect::<std::collections::HashSet<_>>();
            if referenced_licenses.len() == 1 {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License(referenced_licenses.into_iter().next().unwrap()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.into()),
                });
            }
        }
        Err(debian_copyright::Error::IoError(e)) => {
            unreachable!("IO error: {}", e);
        }
        Err(debian_copyright::Error::ParseError(e)) => {
            return Err(ProviderError::ParseError(e.to_string()));
        }
        Err(debian_copyright::Error::NotMachineReadable) => {
            for line in text.lines() {
                if let Some(name) = line.strip_prefix("Upstream-Name: ") {
                    ret.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Name(name.to_string()),
                        certainty: Some(Certainty::Possible),
                        origin: Some(Origin::Path(path.into())),
                    });
                }

                if let Some(url) = lazy_regex::regex_find!(r".* was downloaded from ([^\s]+)", line)
                {
                    urls.push(url.to_string());
                    ret.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Download(url.to_string()),
                        certainty: Some(Certainty::Possible),
                        origin: Some(path.into()),
                    });
                }
            }
        }
    }

    for url in urls.into_iter() {
        if let Ok(url) = url.parse() {
            if let Some(repo_url) = crate::vcs::guess_repo_from_url(&url, None) {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(repo_url),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.into()),
                });
            }
        }

        ret.extend(crate::metadata_from_url(
            url.as_str(),
            &Origin::Path(path.into()),
        ));
    }

    Ok(ret)
}

pub fn guess_from_debian_watch(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut ret = vec![];
    use debian_changelog::ChangeLog;
    use debian_watch::{Mode, WatchFile};

    let get_package_name = || -> String {
        let text = std::fs::read_to_string(path.parent().unwrap().join("changelog")).unwrap();
        let cl: ChangeLog = text.parse().unwrap();
        let first_entry = cl.entries().next().unwrap();
        first_entry.package().unwrap()
    };

    let w: WatchFile = std::fs::read_to_string(path)?
        .parse()
        .map_err(|e| ProviderError::ParseError(format!("Failed to parse debian/watch: {}", e)))?;

    let origin = Origin::Path(path.into());

    for entry in w.entries() {
        let url = entry.format_url(get_package_name);
        match entry.mode().unwrap_or_default() {
            Mode::Git => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(url.to_string()),
                    certainty: Some(Certainty::Confident),
                    origin: Some(origin.clone()),
                });
            }
            Mode::Svn => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(url.to_string()),
                    certainty: Some(Certainty::Confident),
                    origin: Some(origin.clone()),
                });
            }
            Mode::LWP => {
                if url.scheme() == "http" || url.scheme() == "https" {
                    if let Some(repo) = crate::vcs::guess_repo_from_url(&url, None) {
                        ret.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Repository(repo),
                            certainty: Some(Certainty::Confident),
                            origin: Some(origin.clone()),
                        });
                    }
                }
            }
        };
        ret.extend(crate::metadata_from_url(url.as_str(), &origin));
    }
    Ok(ret)
}

pub fn debian_is_native(path: &Path) -> std::io::Result<Option<bool>> {
    let format_file_path = path.join("source/format");
    match File::open(format_file_path) {
        Ok(mut file) => {
            let mut content = String::new();
            file.read_to_string(&mut content)?;
            return Ok(Some(content.trim() == "3.0 (native)"));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    let changelog_file = path.join("changelog");
    match File::open(changelog_file) {
        Ok(mut file) => {
            let cl = debian_changelog::ChangeLog::read(&mut file)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            let first_entry = cl.entries().next().unwrap();
            let version = first_entry.version().unwrap();
            return Ok(Some(version.debian_revision.is_none()));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    Ok(None)
}

#[cfg(test)]
mod watch_tests {
    use super::*;

    #[test]
    fn test_empty() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("watch");
        std::fs::write(
            &path,
            r#"
# Blah
"#,
        )
        .unwrap();
        assert!(guess_from_debian_watch(&path, &GuesserSettings::default())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_simple() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("watch");
        std::fs::write(
            &path,
            r#"version=4
https://github.com/jelmer/dulwich/tags/dulwich-(.*).tar.gz
"#,
        )
        .unwrap();
        assert_eq!(
            vec![UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository("https://github.com/jelmer/dulwich".to_string()),
                certainty: Some(Certainty::Confident),
                origin: Some(path.clone().into())
            }],
            guess_from_debian_watch(&path, &GuesserSettings::default()).unwrap()
        );
    }
}
