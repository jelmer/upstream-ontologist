use crate::vcs;
use crate::GuesserSettings;
use log::warn;

fn parse_command_bytes(command: &[u8]) -> Option<Vec<String>> {
    if command.ends_with(b"\\") {
        warn!(
            "Ignoring command with line break: {}",
            String::from_utf8_lossy(command)
        );
        return None;
    }
    let command_str = match String::from_utf8(command.to_vec()) {
        Ok(s) => s,
        Err(_) => {
            warn!(
                "Ignoring command with non-UTF-8: {}",
                String::from_utf8_lossy(command)
            );
            return None;
        }
    };
    let args: Vec<_> = shlex::split(command_str.as_str())?
        .into_iter()
        .filter(|arg| !arg.trim().is_empty())
        .collect();

    if args.is_empty() {
        None
    } else {
        Some(args)
    }
}

/// Extract the upstream repository URL from a command line that looks like
/// `git clone <url>`, `fossil clone <url>`, `cvs -d <cvsroot> co <module>` or
/// `svn co <url>`.
pub fn url_from_vcs_command(command: &[u8]) -> Option<String> {
    if let Some(url) = url_from_git_clone_command(command) {
        return Some(url);
    }
    if let Some(url) = url_from_fossil_clone_command(command) {
        return Some(url);
    }
    if let Some(url) = url_from_cvs_co_command(command) {
        return Some(url);
    }
    if let Some(url) = url_from_svn_co_command(command) {
        return Some(url);
    }
    None
}

/// Extract the upstream repository URL from a command line that looks like
/// `git clone <url>`.
pub fn url_from_git_clone_command(command: &[u8]) -> Option<String> {
    let mut args = parse_command_bytes(command)?;
    if args.remove(0) != "git" || args.remove(0) != "clone" {
        return None;
    }
    let mut i = 0;
    while i < args.len() {
        if !args[i].starts_with('-') {
            i += 1;
            continue;
        }
        if args[i].contains('=') {
            args.remove(i);
            continue;
        }
        // arguments that take a parameter
        if args[i] == "-b" || args[i] == "--depth" || args[i] == "--branch" {
            args.remove(i);
            args.remove(i);
            continue;
        }
        args.remove(i);
    }
    let url = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| args.first().cloned().unwrap_or_default());
    if vcs::plausible_url(&url) {
        Some(url)
    } else {
        None
    }
}

#[test]
fn test_url_from_git_clone_command() {
    assert_eq!(
        url_from_git_clone_command(b"git clone https://github.com/foo/bar foo"),
        Some("https://github.com/foo/bar".to_string())
    );

    assert_eq!(
        Some("https://github.com/jelmer/blah".to_string()),
        url_from_git_clone_command(b"git clone https://github.com/jelmer/blah"),
    );

    assert_eq!(
        Some("https://github.com/jelmer/blah".to_string()),
        url_from_git_clone_command(b"git clone https://github.com/jelmer/blah target"),
    );

    assert_eq!(
        Some("https://github.com/jelmer/blah".to_string()),
        url_from_git_clone_command(b"git clone -b foo https://github.com/jelmer/blah target"),
    );

    assert_eq!(None, url_from_git_clone_command(b"git ls-tree"));
}

/// Get the upstream source from a command line that looks like
/// `fossil clone <url>`.
pub fn url_from_fossil_clone_command(command: &[u8]) -> Option<String> {
    let mut args = parse_command_bytes(command)?;
    if args.remove(0) != "fossil" || args.remove(0) != "clone" {
        return None;
    }
    let mut i = 0;
    while i < args.len() {
        if !args[i].starts_with('-') {
            i += 1;
            continue;
        }
        if args[i].contains('=') {
            args.remove(i);
            continue;
        }
        args.remove(i);
    }
    let url = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| args.first().cloned().unwrap_or_default());
    if vcs::plausible_url(&url) {
        Some(url)
    } else {
        None
    }
}

#[test]
fn test_url_from_fossil_clone_command() {
    assert_eq!(
        Some("https://example.com/repo/blah".to_string()),
        url_from_fossil_clone_command(b"fossil clone https://example.com/repo/blah blah.fossil"),
    );
}

/// Get the upstream source from a command line that looks like
/// `cvs -d <cvsroot> co <module>`.
pub fn url_from_cvs_co_command(command: &[u8]) -> Option<String> {
    let mut args = parse_command_bytes(command)?;
    let i = 0;
    let mut cvsroot = None;
    let mut module = None;
    let mut command_seen = false;
    if args.remove(0) != "cvs" {
        return None;
    }
    while i < args.len() {
        if args[i] == "-d" {
            args.remove(i);
            cvsroot = Some(args.remove(i));
            continue;
        }
        if args[i].starts_with("-d") {
            cvsroot = Some(args.remove(i)[2..].to_string());
            continue;
        }
        if command_seen && !args[i].starts_with('-') {
            module = Some(args[i].clone());
        } else if args[i] == "co" || args[i] == "checkout" {
            command_seen = true;
        }
        args.remove(i);
    }
    if let Some(cvsroot) = cvsroot {
        let url = breezyshim::location::cvs_to_url(&cvsroot);
        if let Some(module) = module {
            return Some(url.join(module.as_str()).unwrap().to_string());
        }
        return Some(url.to_string());
    }
    None
}

/// Get the upstream source from a command line that looks like
/// `svn co <url>`.
pub fn url_from_svn_co_command(command: &[u8]) -> Option<String> {
    let args = parse_command_bytes(command)?;
    if args[0] != "svn" || args[1] != "co" {
        return None;
    }
    let url_schemes = ["svn+ssh", "http", "https", "svn"];
    args.into_iter().find(|arg| {
        url_schemes
            .iter()
            .any(|scheme| arg.starts_with(&format!("{}://", scheme)))
    })
}

/// Guess upstream data from a Makefile or other file that contains a
/// command to get the source code.
pub fn guess_from_get_orig_source(
    path: &std::path::Path,
    _settings: &GuesserSettings,
) -> Result<Vec<crate::UpstreamDatumWithMetadata>, crate::ProviderError> {
    let text = std::fs::read(path)?;
    let mut result = Vec::new();

    for line in text.split(|b| *b == b'\n') {
        if let Some(url) = url_from_vcs_command(line) {
            let certainty = if url.contains('$') {
                crate::Certainty::Possible
            } else {
                crate::Certainty::Likely
            };

            result.push(crate::UpstreamDatumWithMetadata {
                datum: crate::UpstreamDatum::Repository(url),
                certainty: Some(certainty),
                origin: Some(path.into()),
            });
        }
    }

    Ok(result)
}
