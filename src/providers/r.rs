//! See https://r-pkgs.org/description.html

use crate::{
    vcs, Certainty, GuesserSettings, Person, ProviderError, UpstreamDatum,
    UpstreamDatumWithMetadata,
};
use log::debug;
use std::fs::File;
use std::io::Read;
use url::Url;

#[cfg(feature = "r-description")]
pub fn guess_from_r_description(
    path: &std::path::Path,
    settings: &GuesserSettings,
) -> std::result::Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    use mailparse::MailHeaderMap;
    let mut file = File::open(path)?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)?;

    let msg =
        mailparse::parse_mail(&contents).map_err(|e| ProviderError::ParseError(e.to_string()))?;

    let headers = msg.get_headers();

    let mut results = Vec::new();

    fn parse_url_entry(entry: &str) -> Option<(&str, Option<&str>)> {
        let mut parts = entry.splitn(2, " (");
        if let Some(url) = parts.next() {
            let label = parts.next().map(|label| label.trim_end_matches(')').trim());
            Some((url.trim(), label))
        } else {
            Some((entry, None))
        }
    }

    if let Some(package) = headers.get_first_value("Package") {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(package),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(repository) = headers.get_first_value("Repository") {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Archive(repository),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(bug_reports) = headers.get_first_value("BugReports") {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::BugDatabase(bug_reports),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(version) = headers.get_first_value("Version") {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Version(version),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(license) = headers.get_first_value("License") {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::License(license),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(title) = headers.get_first_value("Title") {
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Summary(title),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(desc) = headers
        .get_first_header("Description")
        .map(|h| h.get_value_raw())
    {
        let desc = String::from_utf8_lossy(desc);
        let lines: Vec<&str> = desc.split_inclusive('\n').collect();
        if !lines.is_empty() {
            let reflowed = format!("{}{}", lines[0], textwrap::dedent(&lines[1..].concat()));
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Description(reflowed),
                certainty: Some(Certainty::Certain),
                origin: Some(path.into()),
            });
        }
    }

    if let Some(maintainer) = headers.get_first_value("Maintainer") {
        let person = Person::from(maintainer.as_str());
        results.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Maintainer(person),
            certainty: Some(Certainty::Certain),
            origin: Some(path.into()),
        });
    }

    if let Some(url) = headers.get_first_header("URL").map(|h| h.get_value_raw()) {
        let url = String::from_utf8(url.to_vec()).unwrap();
        let entries: Vec<&str> = url
            .split_terminator(|c| c == ',' || c == '\n')
            .map(str::trim)
            .collect();
        let mut urls = Vec::new();

        for entry in entries {
            if let Some((url, label)) = parse_url_entry(entry) {
                urls.push((label, url));
            }
        }

        if urls.len() == 1 {
            results.push(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Homepage(urls[0].1.to_string()),
                certainty: Some(Certainty::Possible),
                origin: Some(path.into()),
            });
        }

        for (label, url) in urls {
            let url = match Url::parse(url) {
                Ok(url) => url,
                Err(_) => {
                    debug!("Invalid URL: {}", url);
                    continue;
                }
            };
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
                } else if let Some(repo_url) = vcs::guess_repo_from_url(&url, None) {
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

    #[test]
    fn test_read() {
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
        let ret = guess_from_r_description(&path, true).unwrap();
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
                    origin: Some(path.into())
                },
            ]
        );
    }
}
