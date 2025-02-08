use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Station {
    pub title: String,
    pub description: String,
    pub dj: String,
    pub url: String,
}

impl Station {
    pub async fn fetch_all() -> Result<Vec<Self>, reqwest::Error> {
        let client = reqwest::Client::new();
        let response = client
            .get("https://somafm.com/channels.json")
            .send()
            .await?;
        let stations: Vec<Station> = response.json().await?;
        Ok(stations)
    }
}
