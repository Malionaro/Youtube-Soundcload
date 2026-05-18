use crate::models::{PlaylistEntry, PlaylistInfo};

pub async fn resolve_spotify_playlist(url: &str) -> Result<PlaylistInfo, String> {
    let mut embed_url = url.to_string();
    if url.contains("/playlist/") {
        embed_url = url.replace("/playlist/", "/embed/playlist/");
    } else if url.contains("/album/") {
        embed_url = url.replace("/album/", "/embed/album/");
    }
    
    let body = tokio::task::spawn_blocking(move || {
        ureq::get(&embed_url)
            .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64 AppleWebKit/537.36)")
            .call()
            .map_err(|e| e.to_string())?
            .into_string()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    
    if let Some(start) = body.find("<script id=\"__NEXT_DATA__\" type=\"application/json\">") {
        let offset = start + 51;
        if let Some(end) = body[offset..].find("</script>") {
            let json_str = &body[offset..offset+end];
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                let mut playlist = PlaylistInfo {
                    title: "Spotify Playlist".to_string(),
                    entries: Vec::new(),
                    total: 0,
                };
                
                if let Some(name) = json["props"]["pageProps"]["state"]["data"]["entity"]["name"].as_str() {
                    playlist.title = name.to_string();
                } else if let Some(title) = json["props"]["pageProps"]["state"]["data"]["entity"]["title"].as_str() {
                    playlist.title = title.to_string();
                }
                
                if let Some(tracks) = json["props"]["pageProps"]["state"]["data"]["entity"]["trackList"].as_array() {
                    for (i, track) in tracks.iter().enumerate() {
                        if let (Some(title), Some(subtitle)) = (track["title"].as_str(), track["subtitle"].as_str()) {
                            let mut clean_title = title.to_string();
                            clean_title = clean_title.replace("&#39;", "'").replace("&amp;", "&");
                            let mut clean_subtitle = subtitle.to_string();
                            clean_subtitle = clean_subtitle.replace("&#39;", "'").replace("&amp;", "&");
                            
                            let track_title = format!("{} - {}", clean_subtitle, clean_title);
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
                playlist.total = playlist.entries.len();
                if !playlist.entries.is_empty() {
                    return Ok(playlist);
                }
            }
        }
    }
    Err("Could not parse Spotify playlist/album".to_string())
}

pub async fn resolve_spotify_url(url: &str) -> Result<String, String> {
    let url_copy = url.to_string();
    let body = tokio::task::spawn_blocking(move || {
        ureq::get(&url_copy)
            .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64 AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36)")
            .call()
            .map_err(|e| e.to_string())?
            .into_string()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    if let Some(start) = body.find("<title>") {
        if let Some(end) = body[start..].find("</title>") {
            let mut title = body[start + 7..start + end].to_string();

            title = title.replace(" | Spotify", "");
            title = title.replace(" - Spotify", "");
            title = title.replace(" - song and lyrics by ", " ");
            title = title.replace(" - song by ", " ");
            title = title.replace(" - playlist by ", " ");
            title = title.replace(" - EP by ", " ");
            title = title.replace(" - album by ", " ");
            title = title.replace(" - single by ", " ");

            title = title
                .replace("&#39;", "'")
                .replace("&amp;", "&")
                .replace("&quot;", "\"")
                .replace("&lt;", "<")
                .replace("&gt;", ">");

            return Ok(format!("ytsearch1:{}", title.trim()));
        }
    }

    Err("Could not extract title from Spotify page".to_string())
}
