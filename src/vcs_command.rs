use crate::vcs;
use crate::GuesserSettings;
use log::warn;

fn parse_command_bytes(command: &[u8]) -> Option<Vec<String>> {
    if command.ends_with(&[b'\\']) {
        warn!("Ignoring command with line break: {:?}", command);
        return None;
    }
    let command_str = match String::from_utf8(command.to_vec()) {
        Ok(s) => s,
        Err(_) => {
            warn!("Ignoring command with non-UTF-8: {:?}", command);
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
        .unwrap_or_else(|| args.get(0).cloned().unwrap_or_default());
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
}

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
        .unwrap_or_else(|| args.get(0).cloned().unwrap_or_default());
    if vcs::plausible_url(&url) {
        Some(url)
    } else {
        None
    }
}

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

pub fn url_from_svn_co_command(command: &[u8]) -> Option<String> {
    let args = parse_command_bytes(command)?;
    if args[0] != "svn" || args[1] != "co" {
        return None;
    }
    let url_schemes = vec!["svn+ssh", "http", "https", "svn"];
    args.into_iter().find(|arg| {
        url_schemes
            .iter()
            .any(|scheme| arg.starts_with(&format!("{}://", scheme)))
    })
}

pub fn guess_from_get_orig_source(
    path: &std::path::Path,
    settings: &GuesserSettings,
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
