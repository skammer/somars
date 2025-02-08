 use serde::Deserialize;

  #[derive(Debug, Deserialize)]
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
      lastPlaying: String,
      playlists: Vec<Playlist>,
  }

  #[derive(Debug, Deserialize)]
  struct ChannelResponse {
      channels: Vec<Channel>,
  }

  use std::collections::HashMap;

  impl Station {
      async fn parse_pls(url: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
          let response = reqwest::get(url).await?;
          let pls_content = response.text().await?;

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
              return Err("No stream URL found in PLS file".into());
          }
          Ok(stream_url)
      }

      pub async fn fetch_all() -> Result<Vec<Self>, Box<dyn std::error::Error + Send + Sync>> {
          let client = reqwest::Client::new();
          let response = client
              .get("https://somafm.com/channels.json")
              .send()
              .await?;
          let response: ChannelResponse = response.json().await?;

          let stations = futures::future::try_join_all(response.channels.into_iter().map(|channel| async move {
              // Find the highest quality mp3 URL
              let url = channel.playlists
                  .iter()
                  .find(|p| p.format == "mp3" && p.quality == "highest")
                  .map(|p| p.url.clone())
                  .unwrap_or_default();

              let stream_url = Self::parse_pls(&url).await?;

              Ok(Station {
                  id: channel.id,
                  title: channel.title,
                  description: channel.description,
                  dj: channel.dj,
                  genre: channel.genre,
                  url: stream_url,
                  image: channel.image,
                  last_playing: channel.lastPlaying,
              })
          })).await?;

          Ok(stations)
      }
  }

// Sample response of the https://somafm.com/channels.json
//
// {
// "channels": [
//   {
//     "id": "7soul",
//     "title": "Seven Inch Soul",
//     "description": "Vintage soul tracks from the original 45 RPM vinyl.",
//     "dj": "Dion Watts Garcia",
//     "djmail": "dion@somafm.com",
//     "genre": "oldies",
//     "image": "https://api.somafm.com/img/7soul120.png",
//     "largeimage": "https://api.somafm.com/logos/256/7soul256.png",
//     "xlimage": "https://api.somafm.com/logos/512/7soul512.png",
//     "twitter": "",
//     "updated": "1396144686",
//     "playlists": [
//       {
//         "url": "https://api.somafm.com/7soul.pls",
//         "format": "mp3",
//         "quality": "highest"
//       },
//       {
//         "url": "https://api.somafm.com/7soul130.pls",
//         "format": "aac",
//         "quality": "highest"
//       },
//       {
//         "url": "https://api.somafm.com/7soul64.pls",
//         "format": "aacp",
//         "quality": "high"
//       },
//       {
//         "url": "https://api.somafm.com/7soul32.pls",
//         "format": "aacp",
//         "quality": "low"
//       }
//     ],
//     "preroll": [],
//     "listeners": "61",
//     "lastPlaying": "Charlie Baker - You Crack Me Up"
//   },
//   {
//     "id": "beatblender",
//     "title": "Beat Blender",
//     "description": "A late night blend of deep-house and downtempo chill.",
//     "dj": "DJ Shawn",
//     "djmail": "shawn@somafm.com",
//     "genre": "electronic",
//     "image": "https://api.somafm.com/img/blender120.png",
//     "largeimage": "https://api.somafm.com/logos/256/beatblender256.png",
//     "xlimage": "https://api.somafm.com/logos/512/beatblender512.png",
//     "twitter": "",
//     "updated": "1718076422",
//     "playlists": [
//       {
//         "url": "https://api.somafm.com/beatblender.pls",
//         "format": "mp3",
//         "quality": "highest"
//       },
//       {
//         "url": "https://api.somafm.com/beatblender130.pls",
//         "format": "aac",
//         "quality": "highest"
//       },
//       {
//         "url": "https://api.somafm.com/beatblender64.pls",
//         "format": "aacp",
//         "quality": "high"
//       },
//       {
//         "url": "https://api.somafm.com/beatblender32.pls",
//         "format": "aacp",
//         "quality": "low"
//       }
//     ],
//     "preroll": [
//       "https://somafm.com/prerolls/beatblender/BeatBlenderID10.m4a",
//       "https://somafm.com/prerolls/beatblender/BeatBlenderID2.m4a",
//       "https://somafm.com/prerolls/beatblender/BeatBlenderID3.m4a",
//       "https://somafm.com/prerolls/beatblender/BeatBlenderID1.m4a"
//     ],
//     "listeners": "85",
//     "lastPlaying": "Kodomo - Concept 13"
//   },
//   ... etc
