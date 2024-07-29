use crate::{ProviderError, UpstreamDatum};

pub fn guess_from_pecl_package(package: &str) -> Result<Vec<UpstreamDatum>, ProviderError> {
    let url = format!("https://pecl.php.net/packages/{}", package);

    let client = reqwest::blocking::Client::builder()
        .user_agent(crate::USER_AGENT)
        // PECL is slow
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap();

    let response = client
        .get(url)
        .send()
        .map_err(|e| ProviderError::Other(e.to_string()))?;

    match response.status() {
        reqwest::StatusCode::NOT_FOUND => {
            return Ok(vec![]);
        }
        status if !status.is_success() => {
            return Err(ProviderError::Other(format!("HTTP error: {}", status)));
        }
        _ => {}
    }

    let body = response
        .text()
        .map_err(|e| ProviderError::Other(e.to_string()))?;

    guess_from_pecl_page(&body)
}

fn find_tags_by_text<'a>(
    document: &'a scraper::Html,
    tag_name: &'a str,
    text: &'a str,
) -> Vec<scraper::ElementRef<'a>> {
    let selector = scraper::Selector::parse(tag_name).unwrap();
    document
        .select(&selector)
        .filter(move |node| node.text().any(|t| t.contains(text)))
        .collect()
}

fn guess_from_pecl_page(body: &str) -> Result<Vec<UpstreamDatum>, ProviderError> {
    use scraper::Html;
    let document = Html::parse_document(body);
    let mut ret = Vec::new();

    let browse_source_selector = find_tags_by_text(&document, "a", "Browse Source")
        .into_iter()
        .next();

    if let Some(node) = browse_source_selector {
        ret.push(UpstreamDatum::RepositoryBrowse(
            node.value().attr("href").unwrap().to_string(),
        ));
    }

    let package_bugs_selector = find_tags_by_text(&document, "a", "Package Bugs")
        .into_iter()
        .next();

    if let Some(node) = package_bugs_selector {
        ret.push(UpstreamDatum::BugDatabase(
            node.value().attr("href").unwrap().to_string(),
        ));
    }

    use scraper::Element;

    let homepage_selector = find_tags_by_text(&document, "th", "Homepage")
        .into_iter()
        .next()
        .unwrap()
        .parent_element()
        .unwrap()
        .select(&scraper::Selector::parse("td a").unwrap())
        .next();

    if let Some(node) = homepage_selector {
        ret.push(UpstreamDatum::Homepage(
            node.value().attr("href").unwrap().to_string(),
        ));
    }

    Ok(ret)
}

pub struct Pecl;

impl Default for Pecl {
    fn default() -> Self {
        Self::new()
    }
}

impl Pecl {
    pub fn new() -> Self {
        Self
    }
}

impl crate::ThirdPartyRepository for Pecl {
    fn name(&self) -> &'static str {
        "Pecl"
    }

    fn max_supported_certainty(&self) -> crate::Certainty {
        crate::Certainty::Certain
    }

    fn supported_fields(&self) -> &'static [&'static str] {
        &["Homepage", "Repository", "Bug-Database"]
    }

    fn guess_metadata(&self, name: &str) -> Result<Vec<UpstreamDatum>, ProviderError> {
        guess_from_pecl_package(name)
    }
}

#[cfg(test)]
mod pecl_tests {
    use super::*;

    #[test]
    fn test_guess_from_pecl_page() {
        let text = include_str!("pecl.html");
        let ret = guess_from_pecl_page(text).unwrap();
        assert_eq!(
            ret,
            vec![
                UpstreamDatum::RepositoryBrowse(
                    "https://github.com/eduardok/libsmbclient-php".to_string()
                ),
                UpstreamDatum::BugDatabase(
                    "https://github.com/eduardok/libsmbclient-php/issues".to_string()
                ),
                UpstreamDatum::Homepage("https://github.com/eduardok/libsmbclient-php".to_string())
            ]
        );
    }
}
