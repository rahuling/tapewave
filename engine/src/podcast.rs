//! Podcast support: fetch an RSS/Atom feed and list its audio enclosures.

use std::io::BufReader;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Episode {
    pub title: String,
    pub audio_url: String,
    pub published: Option<String>,
    pub description: String,
    pub duration: Option<f64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Podcast {
    pub title: String,
    pub episodes: Vec<Episode>,
}

pub fn fetch(feed_url: &str) -> Result<Podcast, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        .build()
        .into();
    let response = agent
        .get(feed_url)
        .call()
        .map_err(|e| format!("request failed: {e}"))?;
    // Bound memory consumed by unusually large or malicious feeds.
    use std::io::Read;
    let reader = BufReader::new(response.into_body().into_reader().take(8 * 1024 * 1024));
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
            let media_audio = entry
                .media
                .iter()
                .flat_map(|m| m.content.iter())
                .filter(|c| {
                    c.content_type
                        .as_ref()
                        .is_none_or(|t| t.as_str().starts_with("audio/"))
                })
                .find(|c| c.url.is_some());
            let enclosure = entry.links.iter().find(|link| {
                link.rel.as_deref() == Some("enclosure")
                    && link
                        .media_type
                        .as_deref()
                        .is_none_or(|mime| mime.starts_with("audio/"))
            });
            let (audio_url, duration) = if let Some(audio) = media_audio {
                (
                    audio.url.as_ref()?.to_string(),
                    audio
                        .duration
                        .or_else(|| entry.media.iter().find_map(|m| m.duration))
                        .map(|d| d.as_secs_f64()),
                )
            } else {
                let link = enclosure?;
                (link.href.clone(), None)
            };
            let description = plain_text(
                &entry
                    .summary
                    .as_ref()
                    .map(|text| text.content.clone())
                    .or_else(|| {
                        entry
                            .content
                            .as_ref()
                            .and_then(|content| content.body.clone())
                    })
                    .unwrap_or_default(),
            );
            Some(Episode {
                title: entry
                    .title
                    .map(|t| t.content)
                    .unwrap_or_else(|| "Untitled".into()),
                audio_url,
                published: entry.published.or(entry.updated).map(|d| d.to_rfc3339()),
                description,
                duration,
            })
        })
        .collect();

    Ok(Podcast { title, episodes })
}

fn plain_text(value: &str) -> String {
    let mut output = String::new();
    let mut tag = false;
    for ch in value.chars().take(16_000) {
        match ch {
            '<' => {
                tag = true;
                output.push(' ');
            }
            '>' => tag = false,
            _ if !tag => output.push(ch),
            _ => {}
        }
    }
    output
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(4000)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn descriptions_are_plain_text_not_executable_markup() {
        assert_eq!(
            plain_text("<p>A &amp; B</p><p>Next line.</p>"),
            "A & B Next line."
        );
    }
}
