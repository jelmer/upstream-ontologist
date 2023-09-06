use crate::{Certainty, UpstreamDatum, UpstreamDatumWithMetadata};
use reqwest::blocking::Client;
use reqwest::header;
use std::io::Read;

fn guess_from_homepage(url: &str) -> Vec<UpstreamDatumWithMetadata> {
    let client = Client::new();
    let mut headers = header::HeaderMap::new();
    headers.insert(
        header::USER_AGENT,
        header::HeaderValue::from_static(USER_AGENT),
    );
    let response = client.get(url).headers(headers).send().unwrap();
    let mut buffer = Vec::new();
    response.read_to_end(&mut buffer).unwrap();

    let entries = guess_from_page(&buffer, url);
    let mut results = Vec::new();
    for (upstream_datum, certainty) in entries {
        results.push(UpstreamDatumWithMetadata {
            datum: upstream_datum,
            certainty: Some(certainty),
            origin: Some(url.to_owned()),
        });
    }
    results
}

fn guess_from_page(text: &[u8], basehref: &url::Url) -> Vec<(UpstreamDatum, Certainty)> {
    html5ever::Parser::from_utf8(text)
        .from_utf8()
        .read_from(&mut text.as_ref())
        .unwrap();
    let soup = match BeautifulSoup::new(str::from_utf8(text)?, "lxml") {
        Ok(soup) => soup,
        Err(_) => {
            return Err(GuessError {
                message: "lxml not available, not parsing README.md".to_owned(),
            })
        }
    };

    let mut results = Vec::new();
    for a in soup.find_all(Name("a")) {
        if let Some(href) = a.get("href") {
            let labels: Vec<Option<&str>> = vec![a.get("aria-label"), a.text()];
            for label in labels.into_iter().flatten() {
                match label.to_lowercase().as_str() {
                    "github" | "git" | "repository" | "github repository" => {
                        let repository_url = basehref.join(href).unwrap();
                        results.push((
                            UpstreamDatum::Repository(repository_url.to_string()),
                            Certainty::Possible,
                        ));
                    }
                    "github bug tracking" | "bug tracker" => {
                        let bug_tracker_url = basehref.join(href).unwrap();
                        results.push((
                            UpstreamDatum::BugDatabase(bug_tracker_url.to_string()),
                            Certainty::Possible,
                        ));
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(results)
}
