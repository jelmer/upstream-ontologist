use crate::{
    bug_database_from_issue_url, repo_url_from_merge_request_url, Certainty, Person, ProviderError,
    UpstreamDatum, UpstreamDatumWithMetadata,
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
    _trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let file = File::open(path)?;
    let reader = std::io::BufReader::new(file);

    let net_access = None;

    let mut upstream_data: Vec<UpstreamDatumWithMetadata> = Vec::new();

    for line in reader.lines().flatten() {
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
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }

            if let Some(repo_url) = repo_url_from_merge_request_url(&forwarded, net_access) {
                upstream_data.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(repo_url.to_string()),
                    certainty: Some(Certainty::Possible),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
        }
    }

    Ok(upstream_data)
}

pub fn metadata_from_itp_bug_body(
    body: &str,
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
                            origin: None,
                        });
                    }
                    "Version" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Version(value.to_string()),
                            certainty: Some(Certainty::Possible),
                            origin: None,
                        });
                    }
                    "Upstream Author" if !value.is_empty() => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Author(vec![Person::from(value)]),
                            certainty: Some(Certainty::Confident),
                            origin: None,
                        });
                    }
                    "URL" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Homepage(value.to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: None,
                        });
                    }
                    "License" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::License(value.to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: None,
                        });
                    }
                    "Description" => {
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Summary(value.to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: None,
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
        origin: None,
    });

    Ok(results)
}

pub fn guess_from_debian_changelog(
    path: &Path,
    _trust_package: bool,
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
        origin: Some(path.to_string_lossy().to_string()),
    });

    if let Some(version) = first_entry.version() {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Version(version.upstream_version),
            certainty: Some(Certainty::Confident),
            origin: Some(path.to_string_lossy().to_string()),
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
            origin: Some(path.to_string_lossy().to_string()),
        });

        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::CargoCrate(crate_name),
            certainty: Some(Certainty::Certain),
            origin: Some(path.to_string_lossy().to_string()),
        });
    }

    if let Some(itp) = find_itp(first_entry.change_lines().collect::<Vec<_>>().as_slice()) {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::DebianITP(itp),
            certainty: Some(Certainty::Certain),
            origin: Some(path.to_string_lossy().to_string()),
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

    metadata_from_itp_bug_body(log[0].body.as_str())
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

pub fn guess_from_debian_rules(path: &Path, _trust_package: bool) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let f = std::fs::File::open(path)?;
    let mf = makefile_lossless::Makefile::read_relaxed(f)
        .map_err(|e| ProviderError::ParseError(format!("Failed to parse debian/rules: {}", e)))?;

    let mut ret = vec![];

    if let Some(variable) = mf.variable_definitions().find(|v| v.name().as_deref() == Some("DEB_UPSTREAM_GIT")) {
        let origin = Some(path.to_string_lossy().to_string());
        let certainty = Some(Certainty::Likely);
        let datum = UpstreamDatum::Repository(variable.raw_value().unwrap());
        ret.push(UpstreamDatumWithMetadata {
            datum,
            certainty,
            origin,
        });
    }

    if let Some(deb_upstream_url) = mf.variable_definitions().find(|v| v.name().as_deref() == Some("DEB_UPSTREAM_URL")) {
        let origin = Some(path.to_string_lossy().to_string());
        let certainty = Some(Certainty::Likely);
        let datum = UpstreamDatum::Download(deb_upstream_url.raw_value().unwrap());
        ret.push(UpstreamDatumWithMetadata {
            datum,
            certainty,
            origin,
        });
    }

    Ok(ret)
}

pub fn guess_from_debian_watch(
    path: &Path,
    _trust_package: bool,
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

    let origin = Some(path.to_string_lossy().to_string());

    for entry in w.entries() {
        let url = entry.format_url(get_package_name);
        match entry.mode().unwrap_or_default() {
            Mode::Git => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(url.to_string()),
                    certainty: Some(Certainty::Confident),
                    origin: origin.clone(),
                });
            }
            Mode::Svn => {
                ret.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(url.to_string()),
                    certainty: Some(Certainty::Confident),
                    origin: origin.clone(),
                });
            }
            Mode::LWP => {
                if url.scheme() == "http" || url.scheme() == "https" {
                    if let Some(repo) = crate::vcs::guess_repo_from_url(&url, None) {
                        ret.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Repository(repo),
                            certainty: Some(Certainty::Likely),
                            origin: origin.clone(),
                        });
                    }
                }
            }
        };
        ret.extend(crate::metadata_from_url(url.as_str(), origin.as_deref()));
    }
    Ok(ret)
}

pub fn debian_is_native(path: &Path) -> std::io::Result<Option<bool>> {
    let format_file_path = path.join("source/format");
    match File::open(format_file_path) {
        Ok(mut file) => {
            let mut content = String::new();
            file.read_to_string(&mut content)?;
            Ok(Some(content.trim() == "3.0 (native)"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
