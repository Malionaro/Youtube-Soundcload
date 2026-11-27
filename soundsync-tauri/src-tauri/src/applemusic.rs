use crate::models::{PlaylistEntry, PlaylistInfo};
use crate::utils::decode_html_entities;
use std::time::Duration;

fn extract_artist_names(value: &serde_json::Value) -> Option<String> {
    if let Some(name) = value.as_str() {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            return Some(decode_html_entities(trimmed));
        }
    }
    if let Some(obj) = value.as_object() {
        if let Some(name) = obj.get("name").and_then(|v| v.as_str()) {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                return Some(decode_html_entities(trimmed));
            }
        }
    }
    if let Some(arr) = value.as_array() {
        let names: Vec<String> = arr
            .iter()
            .filter_map(|item| {
                item.get("name")
                    .and_then(|v| v.as_str())
                    .or_else(|| item.as_str())
                    .map(|s| decode_html_entities(s.trim()))
                    .filter(|s| !s.is_empty())
            })
            .collect();
        if !names.is_empty() {
            return Some(names.join(", "));
        }
    }
    None
}

fn extract_single_track_id(url: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        if let (Some(k), Some(v)) = (it.next(), it.next()) {
            if k == "i" && !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn extract_meta_og_title(body: &str) -> Option<String> {
    let lower = body.to_ascii_lowercase();
    for marker in ["property=\"og:title\"", "name=\"apple:title\""] {
        if let Some(pos) = lower.find(marker) {
            let tag_start = lower[..pos].rfind("<meta")?;
            let tag_end_rel = lower[pos..].find('>')?;
            let tag = &body[tag_start..=pos + tag_end_rel];
            let tag_lower = &lower[tag_start..=pos + tag_end_rel];
            if let Some(content_pos) = tag_lower.find("content=\"") {
                let start = content_pos + 9;
                if let Some(end) = tag[start..].find('"') {
                    let raw = decode_html_entities(&tag[start..start + end]);
                    let cleaned = raw
                        .replace(" on Apple Music", "")
                        .replace(" bei Apple Music", "")
                        .replace(" w Apple Music", "")
                        .replace(" - Apple Music", "")
                        .replace(" – Apple Music", "");
                    let trimmed = cleaned.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }
    None
}

pub async fn resolve_apple_music_playlist(url: &str) -> Result<PlaylistInfo, String> {
    let url_copy = url.to_string();
    let target_track_id = extract_single_track_id(url);
    let body = tokio::task::spawn_blocking(move || {
        ureq::get(&url_copy)
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

    // Flexible scan for any <script ... type="application/ld+json" ...> block
    let lower_body = body.to_ascii_lowercase();
    let mut search_pos = 0;
    while let Some(rel_start) = lower_body[search_pos..].find("<script") {
        let abs_start = search_pos + rel_start;
        let Some(tag_end_rel) = body[abs_start..].find('>') else {
            break;
        };
        let tag_end = abs_start + tag_end_rel;
        let opening_tag_lower = &lower_body[abs_start..=tag_end];
        let content_start = tag_end + 1;
        let Some(close_rel) = lower_body[content_start..].find("</script>") else {
            break;
        };
        let content_end = content_start + close_rel;
        search_pos = content_end + 9;

        if !opening_tag_lower.contains("application/ld+json") {
            continue;
        }

        let json_str = body[content_start..content_end].trim();
        let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) else {
            continue;
        };

        let schema_type = json["@type"].as_str().unwrap_or("");
        if schema_type == "MusicPlaylist" || schema_type == "MusicAlbum" {
            let album_artist = extract_artist_names(&json["byArtist"]);
            let playlist_name = json["name"]
                .as_str()
                .map(|s| decode_html_entities(s.trim()))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Apple Music Playlist".to_string());

            let mut playlist = PlaylistInfo {
                title: playlist_name,
                entries: Vec::new(),
                total: 0,
            };

            if let Some(tracks) = json["track"].as_array() {
                for track in tracks {
                    let track_type = track["@type"].as_str().unwrap_or("");
                    if !track_type.is_empty() && track_type != "MusicRecording" {
                        continue;
                    }
                    if let Some(ref wanted_id) = target_track_id {
                        let track_url = track["url"].as_str().unwrap_or("");
                        let track_id_field = track["@id"].as_str().unwrap_or("");
                        if !track_url.contains(wanted_id) && !track_id_field.contains(wanted_id) {
                            continue;
                        }
                    }
                    let Some(raw_title) = track["name"].as_str() else {
                        continue;
                    };
                    let title = decode_html_entities(raw_title.trim());
                    if title.is_empty() {
                        continue;
                    }
                    let artist = extract_artist_names(&track["byArtist"])
                        .or_else(|| album_artist.clone());
                    let track_title = match artist {
                        Some(a) => format!("{} - {}", a, title),
                        None => title,
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

            playlist.total = playlist.entries.len();
            if !playlist.entries.is_empty() {
                return Ok(playlist);
            }
        } else if schema_type == "MusicRecording" || schema_type == "MusicSong" {
            if let Some(raw_title) = json["name"].as_str() {
                let title = decode_html_entities(raw_title.trim());
                if !title.is_empty() {
                    let track_title = match extract_artist_names(&json["byArtist"]) {
                        Some(a) => format!("{} - {}", a, title),
                        None => title,
                    };
                    let search_query = format!("ytsearch1:{}", track_title);
                    return Ok(PlaylistInfo {
                        title: track_title.clone(),
                        entries: vec![PlaylistEntry {
                            title: track_title,
                            url: search_query,
                            duration: None,
                            thumbnail: None,
                            index: 1,
                        }],
                        total: 1,
                    });
                }
            }
        }
    }

    // Fallback for single Apple Music track links via OpenGraph metadata
    if let Some(og_title) = extract_meta_og_title(&body) {
        let search_query = format!("ytsearch1:{}", og_title);
        return Ok(PlaylistInfo {
            title: og_title.clone(),
            entries: vec![PlaylistEntry {
                title: og_title,
                url: search_query,
                duration: None,
                thumbnail: None,
                index: 1,
            }],
            total: 1,
        });
    }

    Err("Konnte Apple Music Link nicht auswerten. Bitte stelle sicher, dass die Playlist oder der Track öffentlich ist.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_single_track_id_from_query() {
        assert_eq!(
            extract_single_track_id("https://music.apple.com/de/album/test/123?i=456789&l=de"),
            Some("456789".to_string())
        );
        assert_eq!(
            extract_single_track_id("https://music.apple.com/de/album/test/123"),
            None
        );
    }

    #[test]
    fn extracts_artist_names_from_string_object_and_array() {
        assert_eq!(extract_artist_names(&json!("Daft Punk")), Some("Daft Punk".to_string()));
        assert_eq!(
            extract_artist_names(&json!({"name": "Rammstein"})),
            Some("Rammstein".to_string())
        );
        assert_eq!(
            extract_artist_names(&json!([{"name": "A &amp; B"}, {"name": "C"}])),
            Some("A & B, C".to_string())
        );
    }

    #[test]
    fn extracts_meta_og_title_safely_with_multibyte_utf8() {
        let html = format!(
            "<html><head><meta property=\"og:title\" content=\"Über &amp; Äpfel – Live bei Apple Music\" />{}</head></html>",
            "ü".repeat(600)
        );
        assert_eq!(
            extract_meta_og_title(&html),
            Some("Über & Äpfel – Live".to_string())
        );
    }
}
