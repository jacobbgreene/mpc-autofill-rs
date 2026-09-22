//! Reproduces the app's fill sequence (minus sign-in and uploads) against the
//! live site to find which stage times out on this machine.
//!   cargo run -p autofill-automation --example probe_flow

use std::time::Instant;

use autofill_automation::Driver;
use autofill_core::{Cardstock, Details};

/// A minimal solid-color PNG written to a temp path, big enough to look like a
/// real card image to the uploader.
fn make_test_png() -> std::path::PathBuf {
    let path = std::env::temp_dir().join("mpc_probe_card.png");
    // 1x1 red PNG (valid, tiny). The site accepts any image bytes.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
        0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
        0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08,
        0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xE5, 0x27, 0xDE,
        0xFC, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    std::fs::write(&path, PNG).expect("write test png");
    path
}

#[tokio::main]
async fn main() {
    let t = Instant::now();
    let chrome = std::env::var("CHROME").ok();
    eprintln!("CHROME={chrome:?}");

    let driver = match Driver::launch(chrome.as_deref()).await {
        Ok(d) => {
            eprintln!("[{:>7.2?}] launch OK", t.elapsed());
            d
        }
        Err(e) => {
            eprintln!("[{:>7.2?}] launch FAILED: {e}", t.elapsed());
            std::process::exit(1);
        }
    };

    match driver.goto_start().await {
        Ok(()) => eprintln!("[{:>7.2?}] goto_start OK", t.elapsed()),
        Err(e) => eprintln!("[{:>7.2?}] goto_start FAILED: {e}", t.elapsed()),
    }

    let quantity = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(18);
    eprintln!("quantity={quantity}");
    let details = Details { quantity, stock: Cardstock::S30, foil: false };
    match driver.define_project(&details).await {
        Ok(()) => eprintln!("[{:>7.2?}] define_project OK", t.elapsed()),
        Err(e) => {
            eprintln!("[{:>7.2?}] define_project FAILED: {e}", t.elapsed());
            driver.close().await;
            std::process::exit(1);
        }
    }

    match driver.page_to_fronts(details.quantity).await {
        Ok(()) => eprintln!("[{:>7.2?}] page_to_fronts OK", t.elapsed()),
        Err(e) => {
            eprintln!("[{:>7.2?}] page_to_fronts FAILED: {e}", t.elapsed());
            driver.close().await;
            std::process::exit(1);
        }
    }

    // This is where the user's run died: the first upload looks for #uploadId.
    // Watch for the element in the main document AND inside the editor iframe
    // for 30s, then try the real upload path.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let probe = r#"(function(){
            var main = !!document.getElementById('uploadId');
            var inFrame = false, frameErr = null;
            try {
                var f = document.getElementById('sysifm_loginFrame');
                inFrame = !!(f && f.contentWindow && f.contentWindow.document.getElementById('uploadId'));
            } catch(e) { frameErr = String(e); }
            return JSON.stringify({main: main, inFrame: inFrame, frameErr: frameErr,
                readyState: document.readyState, path: location.pathname});
        })()"#;
        match driver.probe_string(probe).await {
            Ok(s) => eprintln!("[{:>7.2?}] uploadId probe: {s}", t.elapsed()),
            Err(e) => eprintln!("[{:>7.2?}] uploadId probe error: {e}", t.elapsed()),
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    let png = make_test_png();
    let slots = [0].into_iter().collect();
    match driver.upload_and_insert(&png, &slots).await {
        Ok(()) => eprintln!("[{:>7.2?}] upload_and_insert OK", t.elapsed()),
        Err(e) => eprintln!("[{:>7.2?}] upload_and_insert FAILED: {e}", t.elapsed()),
    }

    driver.close().await;
    eprintln!("[{:>7.2?}] done", t.elapsed());
}
