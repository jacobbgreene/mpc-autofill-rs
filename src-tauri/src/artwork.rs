use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const API: &str = "https://mpcfill.com";
const PAGE_SIZE: u32 = 24;

#[derive(Deserialize)]
struct DpiResponse {
    results: HashMap<String, RemoteDpi>,
}

#[derive(Deserialize)]
struct RemoteDpi {
    dpi: Option<u32>,
}

/// Fetch original-image metadata by ID; preview dimensions are not print DPI.
/// This also works for existing XML orders, which do not store artwork DPI.
#[tauri::command]
pub async fn artwork_dpi(drive_ids: Vec<String>) -> Result<HashMap<String, Option<u32>>, String> {
    fetch_dpi(API, &drive_ids).await
}

async fn fetch_dpi(api: &str, ids: &[String]) -> Result<HashMap<String, Option<u32>>, String> {
    if ids.len() > 2 * autofill_core::order::PROJECT_MAX_SIZE as usize {
        return Err("too many artwork IDs in one deck".into());
    }
    let mut values: HashMap<_, _> = ids.iter().cloned().map(|id| (id, None)).collect();
    let ids: Vec<_> = ids
        .iter()
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 256
                && id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let client = autofill_core::download::build_client();
    // MPC Autofill's cards endpoint accepts up to 1,000 image IDs per request.
    for batch in ids.chunks(1000) {
        let response = client
            .post(format!("{api}/2/cards/"))
            .timeout(Duration::from_secs(25))
            .header("Content-Type", "application/json")
            .body(json!({ "cardIdentifiers": batch }).to_string())
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        let response: DpiResponse = serde_json::from_str(&response)
            .map_err(|e| format!("MPC Autofill returned unreadable artwork metadata: {e}"))?;
        for id in batch {
            let dpi = response
                .results
                .get(*id)
                .and_then(|card| card.dpi)
                .filter(|dpi| *dpi > 0);
            values.insert((*id).clone(), dpi);
        }
    }
    Ok(values)
}

#[derive(Deserialize)]
struct SearchResponse {
    cards: Vec<RemoteCard>,
    count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dpi_lookup_batches_unique_ids_and_preserves_unknowns() {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut batches = Vec::new();
            for connection in listener.incoming().take(2) {
                let mut connection = connection.unwrap();
                connection
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(&connection);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse::<usize>().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let request: Value = serde_json::from_slice(&body).unwrap();
                let ids = request["cardIdentifiers"].as_array().unwrap();
                batches.push(ids.len());
                let results: HashMap<_, _> = ids
                    .iter()
                    .filter_map(|id| {
                        let id = id.as_str().unwrap();
                        match id {
                            "absent" => None,
                            "zero" => Some((id, json!({"dpi": 0}))),
                            _ => Some((id, json!({"dpi": 1200}))),
                        }
                    })
                    .collect();
                let body = json!({"results": results}).to_string();
                write!(
                    connection,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
            batches
        });
        let mut ids: Vec<String> = (0..1001).map(|i| format!("art-{i}")).collect();
        ids.extend(["art-0", "absent", "zero", "/local/file.png"].map(String::from));
        let values = fetch_dpi(&format!("http://{address}"), &ids).await.unwrap();
        assert_eq!(server.join().unwrap(), vec![1000, 3]);
        assert_eq!(values.len(), 1004);
        assert_eq!(values["art-0"], Some(1200));
        assert_eq!(values["art-1000"], Some(1200));
        assert_eq!(values["zero"], None);
        assert_eq!(values["absent"], None);
        assert_eq!(values["/local/file.png"], None);
    }

    #[tokio::test]
    #[ignore = "requires access to the live MPC Autofill API"]
    async fn live_dpi_lookup_reads_original_smaug_artwork_resolution() {
        let id = "1iHSnZIulG-WsLa60PWG0nNVQJ4u6boke";
        let values = artwork_dpi(vec![id.into()]).await.unwrap();
        assert!(values[id].is_some_and(|dpi| dpi > 0));
    }

    #[tokio::test]
    #[ignore = "requires access to the live MPC Autofill API"]
    async fn live_artwork_search_returns_selectable_drive_images() {
        let results = search_artwork("Lightning Bolt".into(), 0, None)
            .await
            .unwrap();
        assert!(results.count > 0);
        assert!(!results.cards.is_empty());
        assert!(results
            .cards
            .iter()
            .all(|card| !card.drive_id.is_empty() && card.thumb.starts_with("https://")));
    }

    #[tokio::test]
    #[ignore = "requires access to the live MPC Autofill API"]
    async fn live_cardback_search_can_browse_without_a_query() {
        let results = search_artwork(String::new(), 0, Some(SearchKind::Cardback))
            .await
            .unwrap();
        assert!(results.count > 0);
        assert!(!results.cards.is_empty());
        assert!(results
            .cards
            .iter()
            .all(|card| card.card_type == "CARDBACK"));
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteCard {
    card_type: String,
    identifier: String,
    name: String,
    extension: String,
    source_name: String,
    dpi: u32,
    medium_thumbnail_url: String,
}

#[derive(Serialize)]
pub struct ArtworkOption {
    card_type: String,
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

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum SearchKind {
    Card,
    Cardback,
}

#[tauri::command]
pub async fn search_artwork(
    query: String,
    page: u32,
    kind: Option<SearchKind>,
) -> Result<ArtworkResults, String> {
    let query = query.trim();
    let cardbacks = matches!(kind, Some(SearchKind::Cardback));
    if (!cardbacks && query.is_empty()) || query.len() > 300 || page > 1000 {
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
        "query": (!query.is_empty()).then_some(query), "cardTypes": if cardbacks { vec!["CARDBACK"] } else { vec!["CARD", "TOKEN"] },
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
                card_type: card.card_type,
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
