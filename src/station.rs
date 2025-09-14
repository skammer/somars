use serde::Deserialize;
use crate::error::AppError;

#[derive(Debug, Deserialize, Clone)]
pub struct Station {
    pub id: String,
    pub title: String,
    pub description: String,
    pub dj: String,
    pub genre: String,
    pub url: String,
    pub image: String,
    pub last_playing: String,
}

#[derive(Debug, Deserialize)]
struct Playlist {
    url: String,
    format: String,
    quality: String,
}

#[derive(Debug, Deserialize)]
struct Channel {
    id: String,
    title: String,
    description: String,
    dj: String,
    genre: String,
    image: String,
    #[serde(rename = "lastPlaying")]
    last_playing: String,
    playlists: Vec<Playlist>,
}

#[derive(Debug, Deserialize)]
struct ChannelResponse {
    channels: Vec<Channel>,
}

impl Station {
    pub async fn parse_pls(url: &str) -> Result<String, AppError> {
        // Handle empty URLs
        if url.is_empty() {
            return Err(AppError::Station("Empty PLS URL provided".to_string()));
        }
        
        let response = reqwest::get(url).await
            .map_err(|e| AppError::Network(e))?;
            
        // Check if the response is successful
        if !response.status().is_success() {
            return Err(AppError::Station(format!("Failed to fetch PLS file: HTTP {}", response.status())));
        }
        
        let pls_content = response.text().await
            .map_err(|e| AppError::Network(e))?;

        let mut stream_url = String::new();
        for line in pls_content.lines() {
            if line.starts_with("File") {
                if let Some(url) = line.split('=').nth(1) {
                    stream_url = url.trim().to_string();
                    break;
                }
            }
        }

        if stream_url.is_empty() {
            Err(AppError::Station("No stream URL found in PLS file".to_string()))
        } else {
            Ok(stream_url)
        }
    }

    pub async fn fetch_all() -> Result<Vec<Self>, AppError> {
        let client = reqwest::Client::new();
        let response = client
            .get("https://somafm.com/channels.json")
            .send()
            .await
            .map_err(|e| AppError::Network(e))?;
            
        // Check if the response is successful
        if !response.status().is_success() {
            return Err(AppError::Station(format!("Failed to fetch channels: HTTP {}", response.status())));
        }
        
        let response: ChannelResponse = response.json().await
            .map_err(|e| AppError::Network(e))?;

        let stations = futures::future::try_join_all(response.channels.into_iter().map(|channel| async move {
            // Find the highest quality mp3 URL as primary choice
            let mp3_highest = channel.playlists
                .iter()
                .find(|p| p.format == "mp3" && p.quality == "highest")
                .map(|p| p.url.clone());
                
            // Fallback to any mp3 URL
            let mp3_any = channel.playlists
                .iter()
                .find(|p| p.format == "mp3")
                .map(|p| p.url.clone());
                
            // Fallback to any playlist URL
            let any_url = if channel.playlists.is_empty() {
                None
            } else {
                Some(channel.playlists[0].url.clone())
            };

            // Try URLs in order of preference
            let playlist_url = mp3_highest.or(mp3_any).or(any_url)
                .unwrap_or_default();

            // Only try to parse PLS if we have a URL
            let stream_url = if !playlist_url.is_empty() {
                match Self::parse_pls(&playlist_url).await {
                    Ok(url) => url,
                    Err(e) => {
                        eprintln!("Warning: Failed to parse playlist for station {}: {}", channel.id, e);
                        // Return the playlist URL directly as fallback
                        playlist_url.clone()
                    }
                }
            } else {
                eprintln!("Warning: No playlist URL found for station {}", channel.id);
                String::new()
            };

            Ok::<Station, AppError>(Station {
                id: channel.id,
                title: channel.title,
                description: channel.description,
                dj: channel.dj,
                genre: channel.genre,
                url: stream_url,
                image: channel.image,
                last_playing: channel.last_playing,
            })
        })).await?;

        Ok(stations)
    }
}