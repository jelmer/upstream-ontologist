use crate::{Certainty, Origin, ProviderError, UpstreamDatum, UpstreamDatumWithMetadata};

use std::error::Error;

use reqwest::blocking::Client;
use reqwest::header::USER_AGENT;
use scraper::{Html, Selector};

pub fn guess_from_homepage(
    url: &url::Url,
) -> Result<Vec<UpstreamDatumWithMetadata>, ProviderError> {
    let client = Client::new();
    let response = client
        .get(url.clone())
        .header(USER_AGENT, crate::USER_AGENT)
        .send()?;

    let body = response.text()?;
    Ok(guess_from_page(&body, url))
}

fn guess_from_page(text: &str, basehref: &url::Url) -> Vec<UpstreamDatumWithMetadata> {
    let fragment = Html::parse_document(text);
    let selector = Selector::parse("a").unwrap();

    let mut result = Vec::new();

    for element in fragment.select(&selector) {
        if let Some(href) = element.value().attr("href") {
            let labels: Vec<String> = vec![
                element.value().attr("aria-label").unwrap_or("").to_string(),
                element.text().collect::<String>(),
            ];
            for label in labels.iter().filter(|&label| !label.is_empty()) {
                match label.to_lowercase().as_str() {
                    "github" | "git" | "repository" | "github repository" => {
                        result.push(UpstreamDatumWithMetadata {
                            origin: Some(Origin::Url(basehref.clone())),
                            datum: UpstreamDatum::Repository(
                                basehref.join(href).unwrap().to_string(),
                            ),
                            certainty: Some(Certainty::Possible),
                        });
                    }
                    "github bug tracking" | "bug tracker" => {
                        result.push(UpstreamDatumWithMetadata {
                            origin: Some(Origin::Url(basehref.clone())),
                            datum: UpstreamDatum::Repository(
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
