use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const API: &str = "https://mpcfill.com";
const PAGE_SIZE: u32 = 24;

#[derive(Deserialize)]
struct SearchResponse {
    cards: Vec<RemoteCard>,
    count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires access to the live MPC Autofill API"]
    async fn live_artwork_search_returns_selectable_drive_images() {
        let results = search_artwork("Lightning Bolt".into(), 0).await.unwrap();
        assert!(results.count > 0);
        assert!(!results.cards.is_empty());
        assert!(results
            .cards
            .iter()
            .all(|card| !card.drive_id.is_empty() && card.thumb.starts_with("https://")));
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteCard {
    identifier: String,
    name: String,
    extension: String,
    source_name: String,
    dpi: u32,
    medium_thumbnail_url: String,
}

#[derive(Serialize)]
pub struct ArtworkOption {
    drive_id: String,
    name: String,
    label: String,
    source: String,
    dpi: u32,
    thumb: String,
}

#[derive(Serialize)]
pub struct ArtworkResults {
    cards: Vec<ArtworkOption>,
    count: u32,
    next_page: Option<u32>,
}

#[tauri::command]
pub async fn search_artwork(query: String, page: u32) -> Result<ArtworkResults, String> {
    let query = query.trim();
    if query.is_empty() || query.len() > 300 || page > 1000 {
        return Err("enter a card name to search".into());
    }
    let client = autofill_core::download::build_client();
    let sources: Value = serde_json::from_str(
        &client
            .get(format!("{API}/2/sources/"))
            .timeout(Duration::from_secs(25))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("MPC Autofill returned an unreadable source list: {e}"))?;
    let sources: Vec<Value> = sources
        .get("results")
        .and_then(Value::as_object)
        .ok_or("MPC Autofill returned no artwork sources")?
        .iter()
        .filter(|(_, source)| source["sourceType"] == "Google Drive")
        .filter_map(|(id, _)| id.parse::<u64>().ok().map(|id| json!([id, true])))
        .collect();
    let body = json!({
        "query": query, "cardTypes": ["CARD", "TOKEN"],
        "pageSize": PAGE_SIZE, "pageStart": page * PAGE_SIZE, "sortBy": "nameAscending",
        "searchSettings": {
            "sourceSettings": { "sources": sources },
            "filterSettings": { "minimumDPI": 0, "maximumDPI": 1500, "maximumSize": 30,
                "languages": [], "includesTags": [], "excludesTags": [] },
            "searchTypeSettings": { "fuzzySearch": false, "filterCardbacks": false }
        }
    });
    let response = client
        .post(format!("{API}/2/exploreSearch/"))
        .timeout(Duration::from_secs(30))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let response: SearchResponse = serde_json::from_str(&response)
        .map_err(|e| format!("MPC Autofill returned an unreadable search result: {e}"))?;
    Ok(ArtworkResults {
        next_page: ((page + 1) * PAGE_SIZE < response.count).then_some(page + 1),
        count: response.count,
        cards: response
            .cards
            .into_iter()
            .map(|card| ArtworkOption {
                // Include the image ID to keep two artists' same-named files distinct.
                name: format!("{} ({}).{}", card.name, card.identifier, card.extension),
                drive_id: card.identifier,
                label: card.name,
                source: card.source_name,
                dpi: card.dpi,
                thumb: card.medium_thumbnail_url,
            })
            .collect(),
    })
}
