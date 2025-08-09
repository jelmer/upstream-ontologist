use crate::{Certainty, Origin, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};

use select::document::Document;
use select::predicate::Name;

/// Guesses upstream metadata by analyzing a project's homepage
pub async fn guess_from_homepage(
    url: &url::Url,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let client = crate::http::build_client().build().unwrap();
    let response = client.get(url.as_str()).send().await?;

    let body = response.text().await?;
    Ok(guess_from_page(&body, url))
}

fn guess_from_page(text: &str, basehref: &url::Url) -> Vec<UpstreamDatumWithMetadata> {
    let fragment = Document::from(text);

    let mut result = Vec::new();
    let origin = Some(Origin::Url(basehref.clone()));

    for element in fragment.find(Name("a")) {
        if let Some(href) = element.attr("href") {
            let labels: Vec<Option<String>> = vec![
                element.attr("aria-label").map(|s| s.to_string()),
                Some(element.text().trim().to_string()),
            ];
            for label in labels.iter().filter_map(|x| x.as_ref()) {
                match label.to_lowercase().as_str() {
                    "github" | "git" | "repository" | "github repository" => {
                        result.push(UpstreamDatumWithMetadata {
                            origin: origin.clone(),
                            datum: UpstreamDatum::Repository(
                                basehref.join(href).unwrap().to_string(),
                            ),
                            certainty: Some(Certainty::Possible),
                        });
                    }
                    "github bug tracking" | "bug tracker" => {
                        result.push(UpstreamDatumWithMetadata {
                            origin: origin.clone(),
                            datum: UpstreamDatum::BugDatabase(
                                basehref.join(href).unwrap().to_string(),
                            ),
                            certainty: Some(Certainty::Possible),
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guess_from_page() {
        let basehref = url::Url::parse("https://example.com").unwrap();
        let text = r#"
            <html>
                <body>
                    <a href="https://github.com/owner/repo">GitHub</a>
                    <a href="https://git.samba.org/samba.org">repository</a>

                    And here is a link with an aria-label:
                    <a href="https://bugs.debian.org/123" aria-label="bug tracker">Debian bug tracker</a>
                </body>
            </html>
        "#;
        let result = guess_from_page(text, &basehref);
        assert_eq!(
            result,
            vec![
                UpstreamDatumWithMetadata {
                    origin: Some(Origin::Url(basehref.clone())),
                    datum: UpstreamDatum::Repository("https://github.com/owner/repo".to_string()),
                    certainty: Some(Certainty::Possible),
                },
                UpstreamDatumWithMetadata {
                    origin: Some(Origin::Url(basehref.clone())),
                    datum: UpstreamDatum::Repository("https://git.samba.org/samba.org".to_string()),
                    certainty: Some(Certainty::Possible),
                },
                UpstreamDatumWithMetadata {
                    origin: Some(Origin::Url(basehref.clone())),
                    datum: UpstreamDatum::BugDatabase("https://bugs.debian.org/123".to_string()),
                    certainty: Some(Certainty::Possible),
                },
            ]
        );
    }
}
