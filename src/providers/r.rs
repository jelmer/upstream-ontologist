//! See <https://r-pkgs.org/description.html>

use crate::{
    vcs, Certainty, GuesserSettings, Person, ProviderError, UpstreamDatum,
    UpstreamDatumWithMetadata,
};

#[cfg(feature = "r-description")]
pub async fn guess_from_r_description(
    path: &std::path::Path,
    _settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    use std::str::FromStr;
    let contents = std::fs::read_to_string(path)?;

    // TODO: Use parse_relaxed
    let msg = r_description::lossy::RDescription::from_str(&contents)
        .map_err(|e| ProviderError::ParseError(e.to_string()))?;

    let mut results = Vec::new();

    results.push(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name(msg.name),
        certainty: Some(Certainty::Certain),
        origin: Some(path.into()),
    });

    if let Some(repository) = msg.repository {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Archive(repository),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(bug_reports) = msg.bug_reports {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::BugDatabase(bug_reports.to_string()),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    results.push(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Version(msg.version.to_string()),
        certainty: Some(Certainty::Certain),
        origin: Some(path.into()),
    });

    results.push(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::License(msg.license),
        certainty: Some(Certainty::Certain),
        origin: Some(path.into()),
    });

    results.push(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Summary(msg.title),
        certainty: Some(Certainty::Certain),
        origin: Some(path.into()),
    });

    let lines: Vec<&str> = msg.description.split_inclusive('\n').collect();
    if !lines.is_empty() {
        let reflowed = format!("{}{}", lines[0], textwrap::dedent(&lines[1..].concat()));
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Description(reflowed),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(maintainer) = msg.maintainer {
        let person = Person::from(maintainer.as_str());
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Maintainer(person),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(urls) = msg.url {
        if urls.len() == 1 {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Homepage(urls[0].url.to_string()),
                certainty: Some(Certainty::Possible),
                origin: Some(path.into()),
            });
        }

        for entry in urls {
            let url = &entry.url;
            let label = entry.label.as_deref();
            if let Some(hostname) = url.host_str() {
                if hostname == "bioconductor.org" {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Archive("Bioconductor".to_string()),
                        certainty: Some(Certainty::Confident),
                        origin: Some(path.into()),
                    });
                }

                if label.map(str::to_lowercase).as_deref() == Some("devel")
                    || label.map(str::to_lowercase).as_deref() == Some("repository")
                {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Repository(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                } else if label.map(str::to_lowercase).as_deref() == Some("homepage") {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Homepage(url.to_string()),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                } else if let Some(repo_url) = vcs::guess_repo_from_url(url, None).await {
                    results.push(UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Repository(repo_url),
                        certainty: Some(Certainty::Certain),
                        origin: Some(path.into()),
                    });
                }
            }
        }
    }

    Ok(results)
}

#[cfg(test)]
#[cfg(feature = "r-description")]
mod description_tests {
    use super::*;

    #[tokio::test]
    async fn test_read() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("DESCRIPTION");

        std::fs::write(
            &path,
            r#"Package: crul
Title: HTTP Client
Description: A simple HTTP client, with tools for making HTTP requests,
    and mocking HTTP requests. The package is built on R6, and takes
    inspiration from Ruby's 'faraday' gem (<https://rubygems.org/gems/faraday>)
    The package name is a play on curl, the widely used command line tool
    for HTTP, and this package is built on top of the R package 'curl', an
    interface to 'libcurl' (<https://curl.haxx.se/libcurl>).
Version: 0.8.4
License: MIT + file LICENSE
Authors@R: c(
    person("Scott", "Chamberlain", role = c("aut", "cre"),
    email = "myrmecocystus@gmail.com",
    comment = c(ORCID = "0000-0003-1444-9135"))
    )
URL: https://github.com/ropensci/crul (devel)
        https://ropenscilabs.github.io/http-testing-book/ (user manual)
        https://www.example.com/crul (homepage)
BugReports: https://github.com/ropensci/crul/issues
Encoding: UTF-8
Language: en-US
Imports: curl (>= 3.3), R6 (>= 2.2.0), urltools (>= 1.6.0), httpcode
        (>= 0.2.0), jsonlite, mime
Suggests: testthat, fauxpas (>= 0.1.0), webmockr (>= 0.1.0), knitr
VignetteBuilder: knitr
RoxygenNote: 6.1.1
X-schema.org-applicationCategory: Web
X-schema.org-keywords: http, https, API, web-services, curl, download,
        libcurl, async, mocking, caching
X-schema.org-isPartOf: https://ropensci.org
NeedsCompilation: no
Packaged: 2019-08-02 19:58:21 UTC; sckott
Author: Scott Chamberlain [aut, cre] (<https://orcid.org/0000-0003-1444-9135>)
Maintainer: Scott Chamberlain <myrmecocystus@gmail.com>
Repository: CRAN
Date/Publication: 2019-08-02 20:30:02 UTC
"#,
        )
        .unwrap();
        let ret = guess_from_r_description(&path, &GuesserSettings::default())
            .await
            .unwrap();
        assert_eq!(
            ret,
            vec![
                UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Name("crul".to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.clone().into())
                },
                UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Archive("CRAN".to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.clone().into())
                },
                UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::BugDatabase(
                        "https://github.com/ropensci/crul/issues".to_string()
                    ),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.clone().into()),
                },
                UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Version("0.8.4".to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.clone().into())
                },
                UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::License("MIT + file LICENSE".to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.clone().into())
                },
                UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Summary("HTTP Client".to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.clone().into())
                },
                UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Description(
                        r#"A simple HTTP client, with tools for making HTTP requests,
and mocking HTTP requests. The package is built on R6, and takes
inspiration from Ruby's 'faraday' gem (<https://rubygems.org/gems/faraday>)
The package name is a play on curl, the widely used command line tool
for HTTP, and this package is built on top of the R package 'curl', an
interface to 'libcurl' (<https://curl.haxx.se/libcurl>)."#
                            .to_string()
                    ),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.clone().into()),
                },
                UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Maintainer(Person {
                        name: Some("Scott Chamberlain".to_string()),
                        email: Some("myrmecocystus@gmail.com".to_string()),
                        url: None
                    }),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.clone().into()),
                },
                UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(
                        "https://github.com/ropensci/crul".to_string()
                    ),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.clone().into()),
                },
                UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Homepage("https://www.example.com/crul".to_string()),
                    certainty: Some(Certainty::Certain),
                    origin: Some(path.clone().into())
                },
            ]
        );
    }
}
