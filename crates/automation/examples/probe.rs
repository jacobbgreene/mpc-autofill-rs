//! Environment probe: launch Chrome the way the app does and time each CDP
//! operation, so we can see exactly which step is slow or timing out on this
//! machine. Not part of the shipped product — run with:
//!   cargo run -p autofill-automation --example probe [--headful]

use std::time::Instant;

use chromiumoxide::{Browser, BrowserConfig};
use futures::StreamExt;

#[tokio::main]
async fn main() {
    let headful = std::env::args().any(|a| a == "--headful");
    let chrome = std::env::var("CHROME").ok();
    eprintln!("CHROME={chrome:?} headful={headful}");

    let t = Instant::now();
    let mut builder = BrowserConfig::builder().window_size(1200, 900).viewport(None);
    if headful {
        builder = builder.with_head();
    }
    if let Some(path) = chrome.as_deref() {
        builder = builder.chrome_executable(path);
    }
    let config = builder.build().expect("browser config");

    let (mut browser, mut handler) = match Browser::launch(config).await {
        Ok(pair) => {
            eprintln!("[{:>7.2?}] Browser::launch OK", t.elapsed());
            pair
        }
        Err(e) => {
            eprintln!("[{:>7.2?}] Browser::launch FAILED: {e}", t.elapsed());
            std::process::exit(1);
        }
    };
    let handler_task = tokio::spawn(async move { while handler.next().await.is_some() {} });

    let page = match browser
        .new_page("https://www.makeplayingcards.com/design/custom-blank-card.html")
        .await
    {
        Ok(p) => {
            eprintln!("[{:>7.2?}] new_page OK", t.elapsed());
            p
        }
        Err(e) => {
            eprintln!("[{:>7.2?}] new_page FAILED: {e}", t.elapsed());
            std::process::exit(1);
        }
    };

    // Trivial evaluate round-trips — measures raw CDP latency to the tab.
    for i in 0..5 {
        let e = Instant::now();
        match page.evaluate("1 + 1").await {
            Ok(v) => eprintln!(
                "[{:>7.2?}] evaluate #{i} OK in {:?} -> {:?}",
                t.elapsed(),
                e.elapsed(),
                v.value()
            ),
            Err(err) => eprintln!("[{:>7.2?}] evaluate #{i} FAILED in {:?}: {err}", t.elapsed(), e.elapsed()),
        }
    }

    // The app's first real read after landing: the loading-spinner probe.
    let e = Instant::now();
    match page
        .evaluate("!!document.querySelector('.dataLoading') && document.querySelector('.dataLoading').offsetParent !== null")
        .await
    {
        Ok(v) => eprintln!("[{:>7.2?}] spinner probe OK in {:?} -> {:?}", t.elapsed(), e.elapsed(), v.value()),
        Err(err) => eprintln!("[{:>7.2?}] spinner probe FAILED in {:?}: {err}", t.elapsed(), e.elapsed()),
    }

    // Element lookup, as find_element does for #uploadId etc.
    let e = Instant::now();
    match page.find_element("body").await {
        Ok(_) => eprintln!("[{:>7.2?}] find_element(body) OK in {:?}", t.elapsed(), e.elapsed()),
        Err(err) => eprintln!("[{:>7.2?}] find_element FAILED in {:?}: {err}", t.elapsed(), e.elapsed()),
    }

    let _ = browser.close().await;
    handler_task.abort();
    eprintln!("[{:>7.2?}] done", t.elapsed());
}
