use crate::UpstreamDatum;

#[allow(dead_code)]
#[derive(serde::Deserialize)]
struct Contents {
    name: String,
    path: String,
    sha: String,
    size: u64,
    url: url::Url,
    html_url: url::Url,
    git_url: url::Url,
    download_url: Option<url::Url>,
    r#type: String,
    content: Option<String>,
    encoding: Option<String>,
    _links: Links,
}

#[allow(dead_code)]
#[derive(serde::Deserialize)]
struct Links {
    #[serde(rename = "self")]
    self_: String,
    git: url::Url,
    html: url::Url,
}

pub fn guess_from_gobo(package: &str) -> Result<Vec<UpstreamDatum>, crate::ProviderError> {
    let packages_url = "https://api.github.com/repos/gobolinux/Recipes/contents"
        .parse()
        .unwrap();
    let contents: Vec<Contents> =
        serde_json::from_value(crate::load_json_url(&packages_url, None)?).unwrap();

    let package = match contents
        .iter()
        .find(|p| p.name.to_ascii_lowercase() == package.to_ascii_lowercase())
    {
        Some(p) => p,
        None => {
            log::debug!("No gobo package named {}", package);
            return Ok(Vec::new());
        }
    };

    let versions: Vec<Contents> =
        serde_json::from_value(crate::load_json_url(&package.url, None)?).unwrap();

    let last_version = if let Some(last_version) = versions.last() {
        &last_version.name
    } else {
        log::debug!("No versions for gobo package {}", package.name);
        return Ok(Vec::new());
    };

    let base_url: url::Url = format!(
        "https://raw.githubusercontent.com/gobolinux/Recipes/master/{}/{}/",
        package.name, last_version
    )
    .parse()
    .unwrap();
    let client = reqwest::blocking::Client::builder()
        .user_agent(crate::USER_AGENT)
        .build()
        .unwrap();

    let mut result = Vec::new();
    let recipe_url = base_url.join("Recipe").unwrap();
    match client.get(recipe_url.as_ref()).send() {
        Ok(response) => {
            let text = response.text().unwrap();
            for line in text.lines() {
                if let Some(url) = line.strip_prefix("url=") {
                    result.push(UpstreamDatum::Homepage(url.to_string()));
                }
            }
        }
        Err(e) => {
            if e.status() == Some(reqwest::StatusCode::NOT_FOUND) {
                log::error!("No recipe for existing gobo package {}", package.name);
            } else if e.status() == Some(reqwest::StatusCode::FORBIDDEN) {
                log::debug!("error loading {}: {}. rate limiting?", recipe_url, e);
            } else {
                return Err(crate::ProviderError::Other(e.to_string()));
            }
        }
    }

    let description_url = base_url.join("Resources/Description").unwrap();
    match client.get(description_url.as_ref()).send() {
        Ok(response) => {
            for line in response.text().unwrap().lines() {
                if let Some((_, key, value)) = lazy_regex::regex_captures!("\\[(.*)\\] (.*)", line)
                {
                    match key {
                        "Name" => result.push(UpstreamDatum::Name(value.to_string())),
                        "Summary" => result.push(UpstreamDatum::Summary(value.to_string())),
                        "License" => result.push(UpstreamDatum::License(value.to_string())),
                        "Description" => result.push(UpstreamDatum::Description(value.to_string())),
                        "Homepage" => result.push(UpstreamDatum::Homepage(value.to_string())),
                        _ => log::warn!("Unknown field {} in gobo Description", key),
                    }
                }
            }
        }
        Err(e) => {
            if e.status() == Some(reqwest::StatusCode::NOT_FOUND) {
                log::error!("No description for existing gobo package {}", package.name);
            } else if e.status() == Some(reqwest::StatusCode::FORBIDDEN) {
                log::debug!("error loading {}: {}. rate limiting?", description_url, e);
                return Ok(Vec::new());
            } else {
                return Err(crate::ProviderError::Other(e.to_string()));
            }
        }
    }

    Ok(result)
}

pub struct Gobo;

impl Gobo {
    pub fn new() -> Self {
        Self
    }
}

impl crate::ThirdPartyRepository for Gobo {
    fn name(&self) -> &'static str {
        "gobo"
    }

    fn supported_fields(&self) -> &'static [&'static str] {
        &["Homepage", "Repository"][..]
    }

    fn max_supported_certainty(&self) -> crate::Certainty {
        crate::Certainty::Possible
    }

    fn guess_metadata(&self, name: &str) -> Result<Vec<UpstreamDatum>, crate::ProviderError> {
        guess_from_gobo(name)
    }
}
