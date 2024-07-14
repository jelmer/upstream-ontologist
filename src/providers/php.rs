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

struct TextMatches<'a>(&'a str);

impl<'a> select::predicate::Predicate for TextMatches<'a> {
    fn matches(&self, node: &select::node::Node) -> bool {
        node.text() == self.0
    }
}

fn guess_from_pecl_page(body: &str) -> Result<Vec<UpstreamDatum>, ProviderError> {
    use select::document::Document;
    use select::predicate::{And, Name};
    let document =
        Document::from_read(body.as_bytes()).map_err(|e| ProviderError::Other(e.to_string()))?;
    let mut ret = Vec::new();

    if let Some(node) = document
        .find(And(Name("a"), TextMatches("Browse Source")))
        .next()
    {
        ret.push(UpstreamDatum::RepositoryBrowse(
            node.attr("href").unwrap().to_string(),
        ));
    }

    if let Some(node) = document
        .find(And(Name("a"), TextMatches("Package Bugs")))
        .next()
    {
        ret.push(UpstreamDatum::BugDatabase(
            node.attr("href").unwrap().to_string(),
        ));
    }

    if let Some(node) = document
        .find(And(Name("th"), TextMatches("Homepage")))
        .next()
    {
        if let Some(node) = node.parent().and_then(|node| node.find(Name("a")).next()) {
            ret.push(UpstreamDatum::Homepage(
                node.attr("href").unwrap().to_string(),
            ));
        }
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
