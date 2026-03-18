//! See <https://golang.org/doc/modules/gomod-ref>
use crate::{
    Certainty, GuesserSettings, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata,
    UpstreamMetadata,
};
use log::debug;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Extracts upstream metadata from go.mod file
pub fn guess_from_go_mod(
    path: &Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut results = Vec::new();

    for line in reader.lines().map_while(Result::ok) {
        if line.starts_with("module ") {
            let modname = match line.trim().split_once(' ') {
                Some((_, modname)) => modname,
                None => {
                    debug!("Failed to parse module name from line: {}", line);
                    continue;
                }
            };
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(modname.to_owned()),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }
    }

    Ok(results)
}

/// Fetches upstream metadata for a Go package from pkg.go.dev
pub fn remote_go_metadata(package: &str) -> Result<UpstreamMetadata, ProviderError> {
    let mut ret = UpstreamMetadata::default();
    if package.starts_with("github.com/") {
        ret.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::GoImportPath(package.to_string()),
            certainty: Some(Certainty::Certain),
            origin: None,
        });

        let parts: Vec<&str> = package.split('/').collect();
        ret.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository(format!("https://{}", parts[..3].join("/"))),
            certainty: Some(Certainty::Certain),
            origin: None,
        });
    }
    Ok(ret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_missing_file() {
        let result = guess_from_go_mod(
            std::path::Path::new("/nonexistent/go.mod"),
            &GuesserSettings::default(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_file() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("go.mod");
        std::fs::write(&path, "").unwrap();
        let result = guess_from_go_mod(&path, &GuesserSettings::default()).unwrap();
        assert_eq!(result, vec![]);
    }

    #[test]
    fn test_truncated_module_line() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("go.mod");
        std::fs::write(&path, "module").unwrap();
        let result = guess_from_go_mod(&path, &GuesserSettings::default()).unwrap();
        assert_eq!(result, vec![]);
    }
}
