pub fn debian_to_upstream_version(version: &str) -> &str {
    // Drop debian-specific modifiers from an upstream version string.
    version.split("+dfsg").next().unwrap_or_default()
}

pub fn upstream_name_to_debian_source_name(mut upstream_name: &str) -> String {
    if let Some((_, _, abbrev)) = lazy_regex::regex_captures!(r"^(.{10,})\((.*)\)", upstream_name) {
        upstream_name = abbrev;
    }

    // Remove "GNU " prefix
    if upstream_name.starts_with("GNU ") {
        upstream_name = &upstream_name["GNU ".len()..];
    }

    // Convert to lowercase and replace characters
    upstream_name
        .to_lowercase()
        .replace(['_', ' ', '/'], "-")
}

pub fn upstream_package_to_debian_source_name(package: &crate::UpstreamPackage) -> String {
    if package.family == "rust" {
        return format!("rust-{}", package.name.to_lowercase());
    } else if package.family == "perl" {
        return format!("lib{}-perl", package.name.to_lowercase().replace("::", "-"));
    } else if package.family == "node" {
        return format!("node-{}", package.name.to_lowercase());
    }

    // If family is not rust, perl, or node, call upstream_name_to_debian_source_name
    upstream_name_to_debian_source_name(package.name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gnu() {
        assert_eq!("lala", upstream_name_to_debian_source_name("GNU Lala"));
    }

    #[test]
    fn test_abbrev() {
        assert_eq!(
            "mun",
            upstream_name_to_debian_source_name("Made Up Name (MUN)")
        );
    }
}

pub fn upstream_package_to_debian_binary_name(package: &crate::UpstreamPackage) -> String {
    if package.family == "rust" {
        return format!("rust-{}", package.name.to_lowercase());
    } else if package.family == "perl" {
        return format!("lib{}-perl", package.name.to_lowercase().replace("::", "-"));
    } else if package.family == "node" {
        return format!("node-{}", package.name.to_lowercase());
    }

    // TODO(jelmer)
    package.name.to_lowercase().replace('_', "-")
}

pub fn valid_debian_package_name(name: &str) -> bool {
    lazy_regex::regex_is_match!("[a-z0-9][a-z0-9+-.]+", name)
}
