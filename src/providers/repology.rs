use std::collections::HashMap;

#[allow(dead_code)]
#[derive(serde::Deserialize)]
struct Project {
    pub name: String,
    pub status: Option<String>,
    pub www: Vec<String>,
    pub licenses: Vec<String>,
    pub summary: Option<String>,
    pub downloads: Vec<String>,
}

pub fn guess_from_repology(repology_project: &str) -> Result<Vec<(String, String)>, crate::ProviderError> {
    let metadata: Vec<Project> = serde_json::from_value(if let Some(value) = crate::get_repology_metadata(repology_project, None) {
        value
    } else {
        return Ok(Vec::new());
    }
        ).unwrap();

    let mut fields = HashMap::new();

    let mut add_field = |name, value, add| {
        *fields.entry(name).or_insert(HashMap::new())
            .entry(value).or_insert(0) += add;
    };

    for entry in metadata {
        let score = if entry.status.as_deref() == Some("outdated") {
            1
        } else {
            10
        };

        for www in entry.www {
            add_field("Homepage", www, score);
        }

            for license in entry.licenses {
                add_field("License", license, score);
        }

        if let Some(summary) = entry.summary {
            add_field("Summary", summary, score);
        }

        for download in entry.downloads {
                add_field("Download", download, score);
        }
    }

    Ok(fields.into_iter().map(|(name, scores)| {
        (name.to_string(), scores.into_iter().max_by_key(|(_, score)| *score).unwrap().0)
    }).collect::<Vec<(String, String)>>())

}
