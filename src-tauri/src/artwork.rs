use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use autofill_core::download::read_response_limited;
use autofill_core::validation::{validate_drive_id, validate_field};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const API: &str = "https://mpcfill.com";
const PAGE_SIZE: u32 = 24;
const MAX_API_BYTES: usize = 4 * 1024 * 1024;
const MAX_SOURCE_RECORDS: usize = 4096;

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
pub async fn artwork_dpi(
    shutdown: tauri::State<'_, crate::shutdown::Shutdown>,
    drive_ids: Vec<String>,
) -> Result<HashMap<String, Option<u32>>, String> {
    shutdown.run(fetch_dpi(API, &drive_ids)).await
}

async fn fetch_dpi(api: &str, ids: &[String]) -> Result<HashMap<String, Option<u32>>, String> {
    if ids.len() > 2 * autofill_core::order::PROJECT_MAX_SIZE as usize {
        return Err("too many artwork IDs in one deck".into());
    }
    for id in ids {
        validate_field(id, "artwork reference").map_err(|error| error.to_string())?;
    }
    let mut values: HashMap<_, _> = ids.iter().cloned().map(|id| (id, None)).collect();
    let ids: Vec<_> = ids
        .iter()
        .filter(|id| validate_drive_id(id).is_ok())
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
            .map_err(|e| e.to_string())?;
        let bytes = read_response_limited(response, MAX_API_BYTES).await?;
        let response: DpiResponse = serde_json::from_slice(&bytes)
            .map_err(|e| format!("MPC Autofill returned unreadable artwork metadata: {e}"))?;
        if response.results.len() > batch.len()
            || response
                .results
                .keys()
                .any(|id| validate_drive_id(id).is_err() || !batch.contains(&id))
        {
            return Err("MPC Autofill returned unexpected artwork metadata IDs".into());
        }
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

    fn json_response(value: Value) -> String {
        let body = value.to_string();
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn fixture_api(responses: Vec<String>) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            for response in responses {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                let mut connection = loop {
                    match listener.accept() {
                        Ok((connection, _)) => break connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "fixture request never arrived"
                            );
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("fixture accept failed: {error}"),
                    }
                };
                connection.set_nonblocking(false).unwrap();
                connection
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(&connection);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse::<usize>().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                connection.write_all(response.as_bytes()).unwrap();
            }
        });
        (format!("http://{address}"), server)
    }

    fn remote_card(id: &str) -> Value {
        json!({"cardType": "CARD", "identifier": id, "name": "Island", "extension": "png", "sourceName": "Artist", "dpi": 1200, "mediumThumbnailUrl": "https://example.com/preview.jpg"})
    }

    #[tokio::test]
    async fn search_rejects_invalid_remote_ids_fields_and_page_counts() {
        let mut long_name = remote_card("art");
        long_name["name"] = json!("a".repeat(autofill_core::validation::MAX_FIELD_BYTES + 1));
        let mut unsafe_preview = remote_card("art");
        unsafe_preview["mediumThumbnailUrl"] = json!("file:///private.png");
        for cards in [
            vec![remote_card("art#/../../escape")],
            vec![remote_card(&"a".repeat(257))],
            vec![long_name],
            vec![unsafe_preview],
            vec![remote_card("art"), remote_card("art")],
            vec![remote_card("art"); PAGE_SIZE as usize + 1],
        ] {
            let count = cards.len();
            let (api, server) = fixture_api(vec![
                json_response(json!({"results": {}})),
                json_response(json!({"cards": cards, "count": count})),
            ]);
            assert!(fetch_artwork_from(&api, "Island".into(), 0, None)
                .await
                .is_err());
            server.join().unwrap();
        }
        let (api, server) = fixture_api(vec![
            json_response(json!({"results": {}})),
            json_response(json!({"cards": [remote_card("valid-art")], "count": 1})),
        ]);
        let results = fetch_artwork_from(&api, "Island".into(), 0, None)
            .await
            .unwrap();
        assert_eq!(results.cards[0].drive_id, "valid-art");
        assert_eq!(results.cards[0].name, "Island (valid-art).png");
        assert_eq!(results.next_page, None);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn all_api_response_paths_enforce_the_byte_limit() {
        let oversized = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            MAX_API_BYTES + 1
        );
        let (api, server) = fixture_api(vec![oversized.clone()]);
        let error = fetch_artwork_from(&api, "Island".into(), 0, None)
            .await
            .err()
            .unwrap();
        assert!(error.contains("byte limit"));
        server.join().unwrap();
        let (api, server) = fixture_api(vec![
            json_response(json!({"results": {}})),
            oversized.clone(),
        ]);
        let error = fetch_artwork_from(&api, "Island".into(), 0, None)
            .await
            .err()
            .unwrap();
        assert!(error.contains("byte limit"));
        server.join().unwrap();
        let (api, server) = fixture_api(vec![oversized]);
        let error = fetch_dpi(&api, &["art".into()]).await.err().unwrap();
        assert!(error.contains("byte limit"));
        server.join().unwrap();
    }

    #[tokio::test]
    async fn local_dpi_references_remain_unknown_without_network_or_filesystem_probes() {
        let ids = vec!["/local/private.png".into(), r"\\server\private.png".into()];
        let values = fetch_dpi("http://127.0.0.1:1", &ids).await.unwrap();
        assert_eq!(values.len(), 2);
        assert!(values.values().all(Option::is_none));
        let (api, server) = fixture_api(vec![json_response(
            json!({"results": {"../escape": {"dpi": 1200}}}),
        )]);
        assert!(fetch_dpi(&api, &["art".into()]).await.is_err());
        server.join().unwrap();
    }

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
        let values = fetch_dpi(API, &[id.into()]).await.unwrap();
        assert!(values[id].is_some_and(|dpi| dpi > 0));
    }

    #[tokio::test]
    #[ignore = "requires access to the live MPC Autofill API"]
    async fn live_artwork_search_returns_selectable_drive_images() {
        let results = fetch_artwork("Lightning Bolt".into(), 0, None)
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
        let results = fetch_artwork(String::new(), 0, Some(SearchKind::Cardback))
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
    shutdown: tauri::State<'_, crate::shutdown::Shutdown>,
    query: String,
    page: u32,
    kind: Option<SearchKind>,
) -> Result<ArtworkResults, String> {
    shutdown.run(fetch_artwork(query, page, kind)).await
}

async fn fetch_artwork(
    query: String,
    page: u32,
    kind: Option<SearchKind>,
) -> Result<ArtworkResults, String> {
    fetch_artwork_from(API, query, page, kind).await
}

async fn fetch_artwork_from(
    api: &str,
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
    let response = client
        .get(format!("{api}/2/sources/"))
        .timeout(Duration::from_secs(25))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let bytes = read_response_limited(response, MAX_API_BYTES).await?;
    let sources: Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("MPC Autofill returned an unreadable source list: {e}"))?;
    let records = sources
        .get("results")
        .and_then(Value::as_object)
        .ok_or("MPC Autofill returned no artwork sources")?;
    if records.len() > MAX_SOURCE_RECORDS {
        return Err("MPC Autofill returned too many artwork sources".into());
    }
    for (id, source) in records {
        if id.len() > 20 || id.parse::<u64>().is_err() {
            return Err("MPC Autofill returned an invalid artwork source ID".into());
        }
        let kind = source
            .get("sourceType")
            .and_then(Value::as_str)
            .ok_or("MPC Autofill returned an invalid artwork source")?;
        validate_field(kind, "artwork source type").map_err(|error| error.to_string())?;
    }
    let sources: Vec<Value> = records
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
        .post(format!("{api}/2/exploreSearch/"))
        .timeout(Duration::from_secs(30))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let bytes = read_response_limited(response, MAX_API_BYTES).await?;
    let response: SearchResponse = serde_json::from_slice(&bytes)
        .map_err(|e| format!("MPC Autofill returned an unreadable search result: {e}"))?;
    if response.cards.len() > PAGE_SIZE as usize || response.count < response.cards.len() as u32 {
        return Err("MPC Autofill returned an invalid artwork page size".into());
    }
    let unique_ids: BTreeSet<_> = response
        .cards
        .iter()
        .map(|card| card.identifier.as_str())
        .collect();
    if unique_ids.len() != response.cards.len() {
        return Err("MPC Autofill returned duplicate artwork IDs".into());
    }
    let cards = response
        .cards
        .into_iter()
        .map(validate_remote_card)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ArtworkResults {
        next_page: (page < 1000 && (page + 1) * PAGE_SIZE < response.count).then_some(page + 1),
        count: response.count,
        cards,
    })
}

fn validate_remote_card(card: RemoteCard) -> Result<ArtworkOption, String> {
    validate_drive_id(&card.identifier).map_err(|error| error.to_string())?;
    for (value, label) in [
        (&card.name, "artwork name"),
        (&card.source_name, "artwork source"),
        (&card.medium_thumbnail_url, "artwork thumbnail URL"),
    ] {
        validate_field(value, label).map_err(|error| error.to_string())?;
    }
    if !matches!(card.card_type.as_str(), "CARD" | "TOKEN" | "CARDBACK")
        || card.extension.is_empty()
        || card.extension.len() > 16
        || !card
            .extension
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric())
        || !card.medium_thumbnail_url.starts_with("https://")
        || card.medium_thumbnail_url.chars().any(char::is_whitespace)
    {
        return Err("MPC Autofill returned invalid artwork metadata".into());
    }
    // Include the image ID to keep two artists' same-named files distinct.
    let name = format!("{} ({}).{}", card.name, card.identifier, card.extension);
    validate_field(&name, "artwork filename").map_err(|error| error.to_string())?;
    Ok(ArtworkOption {
        card_type: card.card_type,
        name,
        drive_id: card.identifier,
        label: card.name,
        source: card.source_name,
        dpi: card.dpi,
        thumb: card.medium_thumbnail_url,
    })
}
