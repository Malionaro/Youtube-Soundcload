use crate::models::{PlaylistEntry, PlaylistInfo};
use crate::utils::decode_html_entities;
use std::time::Duration;

fn normalize_spotify_embed_url(url: &str) -> String {
    let base = url.split('?').next().unwrap_or(url);
    // Strip locale prefixes such as /intl-de/ before building /embed/<kind>/<id>
    for kind in ["playlist", "album"] {
        let needle = format!("/{}/", kind);
        if let Some(idx) = base.find(&needle) {
            let after = &base[idx + needle.len()..];
            let id = after.split('/').next().unwrap_or("").trim();
            if !id.is_empty() {
                return format!("https://open.spotify.com/embed/{}/{}", kind, id);
            }
        }
    }
    url.to_string()
}

pub async fn resolve_spotify_playlist(url: &str) -> Result<PlaylistInfo, String> {
    let embed_url = normalize_spotify_embed_url(url);

    let body = tokio::task::spawn_blocking(move || {
        ureq::get(&embed_url)
            .timeout(Duration::from_secs(15))
            .set(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
            )
            .call()
            .map_err(|e| e.to_string())?
            .into_string()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    if let Some(start) = body.find("<script id=\"__NEXT_DATA__\"") {
        if let Some(tag_end_rel) = body[start..].find('>') {
            let content_start = start + tag_end_rel + 1;
            if let Some(end) = body[content_start..].find("</script>") {
                let json_str = &body[content_start..content_start + end];
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                    let mut playlist = PlaylistInfo {
                        title: "Spotify Playlist".to_string(),
                        entries: Vec::new(),
                        total: 0,
                    };

                    if let Some(name) =
                        json["props"]["pageProps"]["state"]["data"]["entity"]["name"].as_str()
                    {
                        playlist.title = decode_html_entities(name.trim());
                    } else if let Some(title) =
                        json["props"]["pageProps"]["state"]["data"]["entity"]["title"].as_str()
                    {
                        playlist.title = decode_html_entities(title.trim());
                    }

                    if let Some(tracks) =
                        json["props"]["pageProps"]["state"]["data"]["entity"]["trackList"].as_array()
                    {
                        for track in tracks {
                            if let Some(title) = track["title"].as_str() {
                                let clean_title = decode_html_entities(title.trim());
                                if clean_title.is_empty() {
                                    continue;
                                }
                                let clean_subtitle = track["subtitle"]
                                    .as_str()
                                    .map(|s| decode_html_entities(s.trim()))
                                    .unwrap_or_default();

                                let track_title = if clean_subtitle.is_empty() {
                                    clean_title
                                } else {
                                    format!("{} - {}", clean_subtitle, clean_title)
                                };
                                let search_query = format!("ytsearch1:{}", track_title);
                                let next_idx = playlist.entries.len() + 1;

                                playlist.entries.push(PlaylistEntry {
                                    title: track_title,
                                    url: search_query,
                                    duration: None,
                                    thumbnail: None,
                                    index: next_idx,
                                });
                            }
                        }
                    }
                    playlist.total = playlist.entries.len();
                    if !playlist.entries.is_empty() {
                        return Ok(playlist);
                    }
                }
            }
        }
    }
    Err("Could not parse Spotify playlist/album".to_string())
}

pub async fn resolve_spotify_url(url: &str) -> Result<String, String> {
    let url_copy = url.to_string();
    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        // 1. Prefer Spotify oEmbed API (fast, reliable JSON even when web player is JS-rendered)
        let oembed_url = format!(
            "https://open.spotify.com/oembed?url={}",
            urlencoding::encode(&url_copy)
        );
        if let Ok(resp) = ureq::get(&oembed_url)
            .timeout(Duration::from_secs(10))
            .set("User-Agent", "SoundSync-Downloader")
            .call()
        {
            if let Ok(text) = resp.into_string() {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(raw_title) = json["title"].as_str() {
                        let title = decode_html_entities(raw_title.trim());
                        if !title.is_empty() {
                            let author = json["author_name"]
                                .as_str()
                                .map(|s| decode_html_entities(s.trim()))
                                .unwrap_or_default();
                            let query = if !author.is_empty()
                                && !title.to_lowercase().contains(&author.to_lowercase())
                            {
                                format!("{} - {}", author, title)
                            } else {
                                title
                            };
                            return Ok(format!("ytsearch1:{}", query));
                        }
                    }
                }
            }
        }

        // 2. Fallback: scrape HTML <title>
        let body = ureq::get(&url_copy)
            .timeout(Duration::from_secs(15))
            .set(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
            )
            .call()
            .map_err(|e| e.to_string())?
            .into_string()
            .map_err(|e| e.to_string())?;

        if let Some(start) = body.find("<title") {
            if let Some(tag_end_rel) = body[start..].find('>') {
                let title_start = start + tag_end_rel + 1;
                if let Some(end) = body[title_start..].find("</title>") {
                    let mut title = decode_html_entities(&body[title_start..title_start + end]);

                    title = title
                        .replace(" | Spotify", "")
                        .replace(" - Spotify", "")
                        .replace(" - song and lyrics by ", " ")
                        .replace(" - song by ", " ")
                        .replace(" - playlist by ", " ")
                        .replace(" - EP by ", " ")
                        .replace(" - album by ", " ")
                        .replace(" - single by ", " ");

                    let trimmed = title.trim();
                    if !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("spotify") {
                        return Ok(format!("ytsearch1:{}", trimmed));
                    }
                }
            }
        }

        Err("Could not extract title from Spotify page".to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_spotify_locale_and_query_urls() {
        assert_eq!(
            normalize_spotify_embed_url("https://open.spotify.com/intl-de/playlist/37i9dQZF1DXcBWIGoYBM5M?si=123"),
            "https://open.spotify.com/embed/playlist/37i9dQZF1DXcBWIGoYBM5M"
        );
        assert_eq!(
            normalize_spotify_embed_url("https://open.spotify.com/album/4aawyAB9vmqN3uQ7FjRGTy/"),
            "https://open.spotify.com/embed/album/4aawyAB9vmqN3uQ7FjRGTy"
        );
        assert_eq!(
            normalize_spotify_embed_url("https://open.spotify.com/embed/playlist/abc123"),
            "https://open.spotify.com/embed/playlist/abc123"
        );
    }
}
