use crate::{Certainty, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use log::warn;
use std::process::Command;

pub fn guess_from_meson(
    path: &std::path::Path,
    _trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    // TODO(jelmer): consider looking for a meson build directory to call "meson
    // introspect" on
    // TODO(jelmer): mesonbuild is python; consider using its internal functions to parse
    // meson.build?

    let mut command = Command::new("meson");
    command.arg("introspect").arg("--projectinfo").arg(path);
    let output = command.output().map_err(|_| {
        ProviderError::Other("meson not installed; skipping meson.build introspection".to_string())
    })?;
    if !output.status.success() {
        return Err(ProviderError::Other(format!(
            "meson failed to run; exited with code {}",
            output.status.code().unwrap()
        )));
    }
    let project_info: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| ProviderError::Other(format!("Failed to parse meson project info: {}", e)))?;
    let mut results = Vec::new();
    if let Some(descriptive_name) = project_info.get("descriptive_name") {
        if let Some(name) = descriptive_name.as_str() {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(name.to_owned()),
                certainty: Some(Certainty::Certain),
                origin: Some("meson.build".to_owned()),
            });
        }
    }
    if let Some(version) = project_info.get("version") {
        if let Some(version_str) = version.as_str() {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Version(version_str.to_owned()),
                certainty: Some(Certainty::Certain),
                origin: Some("meson.build".to_owned()),
            });
        }
    }
    Ok(results)
}
