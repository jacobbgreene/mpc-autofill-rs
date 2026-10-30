// SPDX-License-Identifier: GPL-3.0-only
// Adapts MPC Autofill by chilli-axe and contributors; see NOTICE.md.
// Rust/Tauri modifications: 2026-09-20 onward; attribution recorded 2026-10-16.
//! Browser automation for the MakePlayingCards family of sites, driven over the
//! Chrome DevTools Protocol via `chromiumoxide`. This is a faithful port of the
//! reference Python tool's Selenium driver: it steps through the editor by
//! calling the site's own internal JavaScript.

pub mod driver;
mod js;
mod profile;
pub mod site;

pub use driver::{Driver, ImageMode};
pub use site::{Site, MAKE_PLAYING_CARDS};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Cdp(#[from] chromiumoxide::error::CdpError),
    #[error(transparent)]
    Core(#[from] autofill_core::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Site(String),
    #[error("{stage}: {source}")]
    Stage {
        stage: &'static str,
        #[source]
        source: Box<Error>,
    },
}

impl Error {
    pub fn stage(stage: &'static str, source: Error) -> Self {
        Self::Stage {
            stage,
            source: Box::new(source),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
