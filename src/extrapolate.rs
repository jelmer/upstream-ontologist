use crate::{Certainty, UpstreamDatum, UpstreamDatumWithMetadata};
use crate::{ProviderError, UpstreamMetadata};
use log::warn;

const DEFAULT_ITERATION_LIMIT: usize = 10;

struct Extrapolation {
    from_fields: &'static [&'static str],
    to_fields: &'static [&'static str],
    cb: fn(&mut UpstreamMetadata, bool) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError>,
}

fn extrapolate_repository_from_homepage(
    upstream_metadata: &mut UpstreamMetadata,
    net_access: bool,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut ret = vec![];

    let homepage = upstream_metadata.get("Homepage").unwrap();

    let url = match homepage.datum.to_url() {
        Some(url) => url,
        None => return {
            warn!("Homepage field is not a URL");
            Ok(vec![])
        }
    };

    if let Some(repo) =
        crate::vcs::guess_repo_from_url(&url, Some(net_access))
    {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository(repo),
            certainty: Some(
                std::cmp::min(homepage.certainty, Some(Certainty::Likely))
                    .unwrap_or(Certainty::Likely),
            ),
            origin: homepage.origin.clone(),
        });
    }
    Ok(ret)
}

fn extrapolate_homepage_from_repository_browse(
    upstream_metadata: &mut UpstreamMetadata,
    net_access: bool,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut ret = vec![];
    let browse_url = upstream_metadata.get("Repository-Browse").unwrap();

    let url = match browse_url.datum.to_url() {
        Some(url) => url,
        None => return {
            warn!("Repository-Browse field is not a URL");
            Ok(vec![])
        }
    };

    // Some hosting sites are commonly used as Homepage
    // TODO(jelmer): Maybe check that there is a README file that
    // can serve as index?
    let forge = crate::find_forge(&url, Some(net_access));
    if forge.is_some() && forge.unwrap().repository_browse_can_be_homepage() {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Homepage(browse_url.datum.as_str().unwrap().to_string()),
            certainty: Some(
                std::cmp::min(browse_url.certainty, Some(Certainty::Possible))
                    .unwrap_or(Certainty::Possible),
            ),
            origin: browse_url.origin.clone(),
        });
    }
    Ok(ret)
}

fn copy_bug_db_field(
    upstream_metadata: &mut UpstreamMetadata,
    _net_access: bool,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut ret = vec![];

    let old_bug_db = upstream_metadata.get("Bugs-Database").unwrap();

    ret.push(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::BugDatabase(old_bug_db.datum.as_str().unwrap().to_string()),
        certainty: old_bug_db.certainty,
        origin: old_bug_db.origin.clone(),
    });
    upstream_metadata.remove("Bugs-Database");

    Ok(ret)
}

fn extrapolate_repository_from_bug_db(
    upstream_metadata: &mut UpstreamMetadata,
    net_access: bool,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let old_value = upstream_metadata.get("Bug-Database").unwrap();
    let url = match old_value.datum.to_url() {
        Some(url) => url,
        None => return {
            warn!("Bug-Database field is not a URL");
            Ok(vec![])
        }
    };
    let repo =
        crate::vcs::guess_repo_from_url(&url, Some(net_access));

    Ok(if let Some(repo) = repo {
        vec![UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository(repo),
            certainty: Some(
                std::cmp::min(old_value.certainty, Some(Certainty::Likely))
                    .unwrap_or(Certainty::Likely),
            ),
            origin: old_value.origin.clone(),
        }]
    } else {
        vec![]
    })
}

fn extrapolate_repository_browse_from_repository(
    upstream_metadata: &mut UpstreamMetadata,
    net_access: bool,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let old_value = upstream_metadata.get("Repository").unwrap();
    let url = match old_value.datum.to_url() {
        Some(url) => url,
        None => return {
            warn!("Repository field is not a URL");
            Ok(vec![])
        }
    };
    let browse_url = crate::vcs::browse_url_from_repo_url(
        &crate::vcs::VcsLocation {
            url,
            branch: None,
            subpath: None,
        },
        Some(net_access),
    );
    Ok(if let Some(browse_url) = browse_url {
        vec![UpstreamDatumWithMetadata {
            datum: UpstreamDatum::RepositoryBrowse(browse_url.to_string()),
            certainty: old_value.certainty,
            origin: old_value.origin.clone(),
        }]
    } else {
        vec![]
    })
}

fn extrapolate_repository_from_repository_browse(
    upstream_metadata: &mut UpstreamMetadata,
    net_access: bool,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let old_value = upstream_metadata.get("Repository-Browse").unwrap();
    let url = match old_value.datum.to_url() {
        Some(url) => url,
        None => return {
            warn!("Repository-Browse field is not a URL");
            Ok(vec![])
        }
    };
    let repo =
        crate::vcs::guess_repo_from_url(&url, Some(net_access));
    Ok(if let Some(repo) = repo {
        vec![UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository(repo),
            certainty: old_value.certainty,
            origin: old_value.origin.clone(),
        }]
    } else {
        vec![]
    })
}

fn extrapolate_bug_database_from_repository(
    upstream_metadata: &mut UpstreamMetadata,
    net_access: bool,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let old_value = upstream_metadata.get("Repository").unwrap();

    let url = match old_value.datum.to_url() {
        Some(url) => url,
        None => return {
            warn!("Repository field is not a URL");
            Ok(vec![])
        }
    };

    Ok(
        if let Some(bug_db_url) = crate::guess_bug_database_url_from_repo_url(
            &url,
            Some(net_access),
        ) {
            vec![UpstreamDatumWithMetadata {
                datum: UpstreamDatum::BugDatabase(bug_db_url.to_string()),
                certainty: Some(
                    std::cmp::min(old_value.certainty, Some(Certainty::Likely))
                        .unwrap_or(Certainty::Likely),
                ),
                origin: old_value.origin.clone(),
            }]
        } else {
            vec![]
        },
    )
}

fn extrapolate_bug_submit_from_bug_db(
    upstream_metadata: &mut UpstreamMetadata,
    net_access: bool,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let old_value = upstream_metadata.get("Bug-Database").unwrap();

    let url = match old_value.datum.to_url() {
        Some(url) => url,
        None => return {
            warn!("Bug-Database field is not a URL");
            Ok(vec![])
        }
    };

    let bug_submit_url = crate::bug_submit_url_from_bug_database_url(
        &url,
        Some(net_access),
    );

    Ok(if let Some(bug_submit_url) = bug_submit_url {
        vec![UpstreamDatumWithMetadata {
            datum: UpstreamDatum::BugSubmit(bug_submit_url.to_string()),
            certainty: old_value.certainty,
            origin: old_value.origin.clone(),
        }]
    } else {
        vec![]
    })
}

fn extrapolate_bug_db_from_bug_submit(
    upstream_metadata: &mut UpstreamMetadata,
    net_access: bool,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let old_value = upstream_metadata.get("Bug-Submit").unwrap();

    let old_value_url = match old_value.datum.to_url() {
        Some(url) => url,
        None => return Ok(vec![]),
    };

    let bug_db_url = crate::bug_database_url_from_bug_submit_url(&old_value_url, Some(net_access));

    Ok(if let Some(bug_db_url) = bug_db_url {
        vec![UpstreamDatumWithMetadata {
            datum: UpstreamDatum::BugDatabase(bug_db_url.to_string()),
            certainty: old_value.certainty,
            origin: old_value.origin.clone(),
        }]
    } else {
        vec![]
    })
}

fn extrapolate_repository_from_download(
    upstream_metadata: &mut UpstreamMetadata,
    net_access: bool,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let old_value = upstream_metadata.get("Download").unwrap();

    let url = match old_value.datum.to_url() {
        Some(url) => url,
        None => return {
            warn!("Download field is not a URL");
            Ok(vec![])
        }
    };

    let repo =
        crate::vcs::guess_repo_from_url(&url, Some(net_access));
    Ok(if let Some(repo) = repo {
        vec![UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository(repo),
            certainty: Some(
                std::cmp::min(old_value.certainty, Some(Certainty::Likely))
                    .unwrap_or(Certainty::Likely),
            ),
            origin: old_value.origin.clone(),
        }]
    } else {
        vec![]
    })
}

fn extrapolate_name_from_repository(
    upstream_metadata: &mut UpstreamMetadata,
    net_access: bool,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let mut ret = vec![];
    let old_value = upstream_metadata.get("Repository").unwrap();
    let url = match old_value.datum.to_url() {
        Some(url) => url,
        None => return {
            warn!("Repository field is not a URL");
            Ok(vec![])
        }
    };
    let repo =
        crate::vcs::guess_repo_from_url(&url, Some(net_access));
    if let Some(repo) = repo {
        let parsed: url::Url = repo.parse().unwrap();
        let name = parsed.path_segments().unwrap().last().unwrap();
        let name = name.strip_suffix(".git").unwrap_or(name);
        if !name.is_empty() {
            ret.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Name(name.to_string()),
                certainty: Some(
                    std::cmp::min(old_value.certainty, Some(Certainty::Likely))
                        .unwrap_or(Certainty::Likely),
                ),
                origin: old_value.origin.clone(),
            });
        }
    }
    Ok(ret)
}

fn extrapolate_security_contact_from_security_md(
    upstream_metadata: &mut UpstreamMetadata,
    net_access: bool,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let repository_url = upstream_metadata.get("Repository").unwrap();
    let security_md_path = upstream_metadata.get("Security-MD").unwrap();

    let url = match repository_url.datum.to_url() {
        Some(url) => url,
        None => return {
            warn!("Repository field is not a URL");
            Ok(vec![])
        }
    };

    let security_url = crate::vcs::browse_url_from_repo_url(
        &crate::vcs::VcsLocation {
            url,
            branch: None,
            subpath: security_md_path.datum.as_str().map(|x| x.to_string()),
        },
        Some(net_access),
    );

    Ok(if let Some(security_url) = security_url {
        vec![UpstreamDatumWithMetadata {
            datum: UpstreamDatum::SecurityContact(security_url.to_string()),
            certainty: std::cmp::min(repository_url.certainty, security_md_path.certainty),
            origin: repository_url.origin.clone(),
        }]
    } else {
        vec![]
    })
}

fn extrapolate_contact_from_maintainer(
    upstream_metadata: &mut UpstreamMetadata,
    _net_access: bool,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let maintainer = upstream_metadata.get("Maintainer").unwrap();

    Ok(vec![UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Contact(maintainer.datum.as_person().unwrap().to_string()),
        certainty: maintainer.certainty,
        origin: maintainer.origin.clone(),
    }])
}

fn consult_homepage(
    upstream_metadata: &mut UpstreamMetadata,
    net_access: bool,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    if !net_access {
        return Ok(vec![]);
    }
    let homepage = upstream_metadata.get("Homepage").unwrap();

    let url = match homepage.datum.to_url() {
        Some(url) => url,
        None => return {
            warn!("Homepage field is not a URL");
            Ok(vec![])
        }
    };

    let mut ret = vec![];

    for mut entry in crate::homepage::guess_from_homepage(&url)? {
        entry.certainty = std::cmp::min(homepage.certainty, entry.certainty);
        ret.push(entry);
    }
    Ok(ret)
}

const EXTRAPOLATIONS: &[Extrapolation] = &[
    Extrapolation {
        from_fields: &["Homepage"],
        to_fields: &["Repository"],
        cb: extrapolate_repository_from_homepage,
    },
    Extrapolation {
        from_fields: &["Repository-Browse"],
        to_fields: &["Homepage"],
        cb: extrapolate_homepage_from_repository_browse,
    },
    Extrapolation {
        from_fields: &["Bugs-Database"],
        to_fields: &["Bug-Database"],
        cb: copy_bug_db_field,
    },
    Extrapolation {
        from_fields: &["Bug-Database"],
        to_fields: &["Repository"],
        cb: extrapolate_repository_from_bug_db,
    },
    Extrapolation {
        from_fields: &["Repository"],
        to_fields: &["Repository-Browse"],
        cb: extrapolate_repository_browse_from_repository,
    },
    Extrapolation {
        from_fields: &["Repository-Browse"],
        to_fields: &["Repository"],
        cb: extrapolate_repository_from_repository_browse,
    },
    Extrapolation {
        from_fields: &["Repository"],
        to_fields: &["Bug-Database"],
        cb: extrapolate_bug_database_from_repository,
    },
    Extrapolation {
        from_fields: &["Bug-Database"],
        to_fields: &["Bug-Submit"],
        cb: extrapolate_bug_submit_from_bug_db,
    },
    Extrapolation {
        from_fields: &["Bug-Submit"],
        to_fields: &["Bug-Database"],
        cb: extrapolate_bug_db_from_bug_submit,
    },
    Extrapolation {
        from_fields: &["Download"],
        to_fields: &["Repository"],
        cb: extrapolate_repository_from_download,
    },
    Extrapolation {
        from_fields: &["Repository"],
        to_fields: &["Name"],
        cb: extrapolate_name_from_repository,
    },
    Extrapolation {
        from_fields: &["Repository", "Security-MD"],
        to_fields: &["Security-Contact"],
        cb: extrapolate_security_contact_from_security_md,
    },
    Extrapolation {
        from_fields: &["Maintainer"],
        to_fields: &["Contact"],
        cb: extrapolate_contact_from_maintainer,
    },
    Extrapolation {
        from_fields: &["Homepage"],
        to_fields: &["Bug-Database", "Repository"],
        cb: consult_homepage,
    },
];

pub fn extrapolate_fields(
    upstream_metadata: &mut UpstreamMetadata,
    net_access: bool,
    iteration_limit: Option<usize>,
) -> Result<(), ProviderError> {
    let iteration_limit = iteration_limit.unwrap_or(DEFAULT_ITERATION_LIMIT);

    let mut changed = true;
    let mut iterations = 0;

    while changed {
        changed = false;

        iterations += 1;

        if iterations > iteration_limit {
            return Err(ProviderError::ExtrapolationLimitExceeded(iteration_limit));
        }

        for extrapolation in EXTRAPOLATIONS {
            let from_fields = extrapolation.from_fields;
            let to_fields = extrapolation.to_fields;
            let cb = extrapolation.cb;
            let from_values = from_fields
                .iter()
                .map(|f| upstream_metadata.get(f))
                .collect::<Vec<_>>();
            if !from_values.iter().all(|v| v.is_some()) {
                log::trace!(
                    "Not enough values for extrapolation from {:?} to {:?}",
                    from_fields,
                    to_fields
                );
                continue;
            }

            let from_values = from_values
                .iter()
                .map(|v| v.unwrap().clone())
                .collect::<Vec<_>>();

            let from_certainties = from_fields
                .iter()
                .map(|f| upstream_metadata.get(f).unwrap().certainty)
                .collect::<Vec<_>>();

            let from_certainty = *from_certainties.iter().min().unwrap();

            let old_to_values: std::collections::HashMap<_, _> = to_fields
                .iter()
                .filter_map(|f| upstream_metadata.get(f).map(|v| (f, v.clone())))
                .collect();

            assert!(old_to_values.values().all(|v| v.certainty.is_some()));

            // If any of the to_fields already exist in old_to_values with a better or same
            // certainty, then we don't need to extrapolate.
            if to_fields.iter().all(|f| {
                old_to_values
                    .get(f)
                    .map(|v| v.certainty >= from_certainty)
                    .unwrap_or(false)
            }) {
                log::trace!(
                    "Not extrapolating from {:?} to {:?} because of certainty ({:?} >= {:?})",
                    from_fields,
                    to_fields,
                    old_to_values
                        .values()
                        .map(|v| v.certainty)
                        .collect::<Vec<_>>(),
                    from_certainty
                );
                continue;
            }

            let extra_upstream_metadata = cb(upstream_metadata, net_access)?;
            let changes = upstream_metadata.update(extra_upstream_metadata.into_iter());

            if !changes.is_empty() {
                log::debug!(
                    "Extrapolating ({:?} ⇒ {:?}) from ({:?})",
                    old_to_values
                        .iter()
                        .map(|(k, v)| format!("{}: {}", k, v.datum))
                        .collect::<Vec<_>>(),
                    changes
                        .iter()
                        .map(|d| format!("{}: {}", d.datum.field(), d.datum))
                        .collect::<Vec<_>>(),
                    from_values
                        .iter()
                        .map(|v| format!(
                            "{}: {} ({})",
                            v.datum.field(),
                            v.datum,
                            v.certainty
                                .map_or_else(|| "unknown".to_string(), |c| c.to_string())
                        ))
                        .collect::<Vec<_>>()
                );
                changed = true;
            }
        }
    }

    Ok(())
}
