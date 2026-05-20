use crate::models::{PlaylistEntry, PlaylistInfo};

pub async fn resolve_apple_music_playlist(url: &str) -> Result<PlaylistInfo, String> {
    let url_copy = url.to_string();
    let body = tokio::task::spawn_blocking(move || {
        ureq::get(&url_copy)
            .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64 AppleWebKit/537.36)")
            .call()
            .map_err(|e| e.to_string())?
            .into_string()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    // Find the application/ld+json block that has MusicPlaylist or MusicAlbum
    let mut search_pos = 0;
    while let Some(start) = body[search_pos..].find("<script type=\"application/ld+json\">") {
        let abs_start = search_pos + start;
        let offset = abs_start + 35;
        if let Some(end) = body[offset..].find("</script>") {
            let json_str = &body[offset..offset+end];
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                let schema_type = json["@type"].as_str().unwrap_or("");
                if schema_type == "MusicPlaylist" || schema_type == "MusicAlbum" {
                    let mut playlist = PlaylistInfo {
                        title: json["name"].as_str().unwrap_or("Apple Music Playlist").to_string(),
                        entries: Vec::new(),
                        total: 0,
                    };

                    if let Some(tracks) = json["track"].as_array() {
                        for (i, track) in tracks.iter().enumerate() {
                            let track_type = track["@type"].as_str().unwrap_or("");
                            if track_type == "MusicRecording" {
                                if let (Some(title), Some(artist_obj)) = (track["name"].as_str(), track["byArtist"].as_object()) {
                                    if let Some(artist_name) = artist_obj.get("name").and_then(|v| v.as_str()) {
                                        let track_title = format!("{} - {}", artist_name, title);
                                        let search_query = format!("ytsearch1:{}", track_title);

                                        playlist.entries.push(PlaylistEntry {
                                            title: track_title,
                                            url: search_query,
                                            duration: None,
                                            thumbnail: None,
                                            index: i + 1,
                                        });
                                    }
                                }
                            }
                        }
                    }
                    playlist.total = playlist.entries.len();
                    if !playlist.entries.is_empty() {
                        return Ok(playlist);
                    }
                }
            }
            search_pos = offset + end;
        } else {
            break;
        }
    }

    Err("Konnte Apple Music Playlist nicht auswerten. Bitte stelle sicher, dass die Playlist öffentlich ist.".to_string())
}
