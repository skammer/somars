 use serde::Deserialize;
 use std::error::Error;

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

  impl Station {
      pub async fn fetch_all() -> Result<Vec<Self>, reqwest::Error> {
          let client = reqwest::Client::new();
          let response = client
              .get("https://somafm.com/channels.json")
              .send()
              .await?;
          let response: ChannelResponse = response.json().await?;

          let stations = response.channels.into_iter().map(|channel| {
              // Find the highest quality mp3 URL
              let url = channel.playlists
                  .iter()
                  .find(|p| p.format == "mp3" && p.quality == "highest")
                  .map(|p| p.url.clone())
                  .unwrap_or_default();

              Station {
                  id: channel.id,
                  title: channel.title,
                  description: channel.description,
                  dj: channel.dj,
                  genre: channel.genre,
                  url,
                  image: channel.image,
                  last_playing: channel.lastPlaying,
              }
          }).collect();

          Ok(stations)
      }
  }
