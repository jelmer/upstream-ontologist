//! Documentation: https://opam.ocaml.org/doc/Manual.html#Package-definitions
use crate::{Certainty, Person, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};
use log::warn;
use opam_file_rs::value::{OpamFileItem, OpamFileSection, ValueKind};
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub fn guess_from_opam(
    path: &Path,
    _trust_package: bool,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut f = File::open(path)?;
    let mut contents = String::new();
    f.read_to_string(&mut contents)?;
    let opam = opam_file_rs::parse(contents.as_str())
        .map_err(|e| ProviderError::ParseError(format!("Failed to parse OPAM file: {:?}", e)))?;
    let mut results: Vec<UpstreamDatumWithMetadata> = Vec::new();

    fn find_item<'a>(section: &'a OpamFileSection, name: &str) -> Option<&'a OpamFileItem> {
        for child in section.section_item.iter() {
            match child {
                OpamFileItem::Variable(_, n, _) if n == name => return Some(child),
                _ => (),
            }
        }
        None
    }

    for entry in opam.file_contents {
        match entry {
            OpamFileItem::Variable(_, name, value) if name == "maintainer" => {
                let value = match value.kind {
                    ValueKind::String(s) => s,
                    _ => {
                        warn!("Unexpected type for maintainer in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Maintainer(Person::from(value.as_str())),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Variable(_, name, value) if name == "license" => {
                let value = match value.kind {
                    ValueKind::String(s) => s,
                    _ => {
                        warn!("Unexpected type for license in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License(value),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Variable(_, name, value) if name == "homepage" => {
                let value = match value.kind {
                    ValueKind::String(s) => s,
                    _ => {
                        warn!("Unexpected type for homepage in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Homepage(value),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Section(_, section)
                if section.section_name.as_deref() == Some("dev-repo") =>
            {
                match find_item(&section, "repository") {
                    Some(OpamFileItem::Variable(_, _, ref value)) => {
                        let value = match value.kind {
                            ValueKind::String(ref s) => s,
                            _ => {
                                warn!("Unexpected type for dev-repo in OPAM file: {:?}", value);
                                continue;
                            }
                        };
                        results.push(UpstreamDatumWithMetadata {
                            datum: UpstreamDatum::Repository(value.to_string()),
                            certainty: Some(Certainty::Confident),
                            origin: Some(path.to_string_lossy().to_string()),
                        });
                    }
                    Some(o) => {
                        warn!("Unexpected type for dev-repo in OPAM file: {:?}", o);
                        continue;
                    }
                    None => {
                        warn!("Missing repository for dev-repo in OPAM file");
                        continue;
                    }
                }
            }
            OpamFileItem::Variable(_, name, value) if name == "bug-reports" => {
                let value = match value.kind {
                    ValueKind::String(s) => s,
                    _ => {
                        warn!("Unexpected type for bug-reports in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::BugDatabase(value),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Variable(_, name, value) if name == "synopsis" => {
                let value = match value.kind {
                    ValueKind::String(s) => s,
                    _ => {
                        warn!("Unexpected type for synopsis in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary(value),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Variable(_, name, value) if name == "description" => {
                let value = match value.kind {
                    ValueKind::String(s) => s,
                    _ => {
                        warn!("Unexpected type for description in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Description(value),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Variable(_, name, value) if name == "doc" => {
                let value = match value.kind {
                    ValueKind::String(s) => s,
                    _ => {
                        warn!("Unexpected type for doc in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Documentation(value),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Variable(_, name, value) if name == "version" => {
                let value = match value.kind {
                    ValueKind::String(s) => s,
                    _ => {
                        warn!("Unexpected type for version in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version(value),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Variable(_, name, value) if name == "authors" => {
                let value = match value.kind {
                    ValueKind::String(s) => vec![Person::from(s.as_str())],
                    ValueKind::List(ref l) => l
                        .iter()
                        .filter_map(|v| match v.kind {
                            ValueKind::String(ref s) => Some(Person::from(s.as_str())),
                            _ => {
                                warn!("Unexpected type for authors in OPAM file: {:?}", &value);
                                None
                            }
                        })
                        .collect(),
                    _ => {
                        warn!("Unexpected type for authors in OPAM file: {:?}", value);
                        continue;
                    }
                };
                results.push(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Author(value),
                    certainty: Some(Certainty::Confident),
                    origin: Some(path.to_string_lossy().to_string()),
                });
            }
            OpamFileItem::Variable(_, name, _) => {
                warn!("Unexpected variable in OPAM file: {}", name);
            }
            OpamFileItem::Section(_, section) => {
                warn!("Unexpected section in OPAM file: {:?}", section);
            }
        }
    }

    Ok(results)
}
