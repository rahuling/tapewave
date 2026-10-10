//! Podcast support: fetch an RSS/Atom feed and list its audio enclosures.

use std::io::BufReader;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Episode {
    pub title: String,
    pub audio_url: String,
    pub published: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Podcast {
    pub title: String,
    pub episodes: Vec<Episode>,
}

pub fn fetch(feed_url: &str) -> Result<Podcast, String> {
    let response = ureq::get(feed_url).call().map_err(|e| format!("request failed: {e}"))?;
    let reader = BufReader::new(response.into_body().into_reader());
    let feed = feed_rs::parser::parse(reader).map_err(|e| format!("not a valid feed: {e}"))?;

    let title = feed
        .title
        .map(|t| t.content)
        .unwrap_or_else(|| feed_url.to_string());

    let episodes = feed
        .entries
        .into_iter()
        .filter_map(|entry| {
            // Podcast audio is attached as a media enclosure.
            let audio_url = entry
                .media
                .iter()
                .flat_map(|m| m.content.iter())
                .find_map(|c| c.url.as_ref().map(|u| u.to_string()))?;
            Some(Episode {
                title: entry.title.map(|t| t.content).unwrap_or_else(|| "Untitled".into()),
                audio_url,
                published: entry.published.or(entry.updated).map(|d| d.to_rfc3339()),
            })
        })
        .collect();

    Ok(Podcast { title, episodes })
}
