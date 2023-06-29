use crate::{Certainty, UpstreamDatum, UpstreamDatumWithMetadata};
use log::warn;
use std::process::Command;

pub fn guess_from_meson(
    path: &std::path::Path,
    _trust_package: bool,
) -> Vec<UpstreamDatumWithMetadata> {
    // TODO(jelmer): consider looking for a meson build directory to call "meson
    // introspect" on
    // TODO(jelmer): mesonbuild is python; consider using its internal functions to parse
    // meson.build?

    let mut command = Command::new("meson");
    command.arg("introspect").arg("--projectinfo").arg(path);
    let output = match command.output() {
        Ok(output) => output,
        Err(_) => {
            warn!("meson not installed; skipping meson.build introspection");
            return Vec::new();
        }
    };
    if !output.status.success() {
        warn!(
            "meson failed to run; exited with code {}",
            output.status.code().unwrap()
        );
        return Vec::new();
    }
    let project_info: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(_) => {
            warn!("Failed to parse meson project info");
            return Vec::new();
        }
    };
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
    results
}
