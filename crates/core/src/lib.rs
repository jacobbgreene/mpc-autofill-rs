//! Pure order-processing logic for the MPC Autofill desktop tool: parsing order
//! XML, downloading card images, and post-processing them. This crate is free of
//! any GUI or browser-automation dependencies so it compiles and tests quickly.

pub mod download;
pub mod decklist;
pub mod new_order;
pub mod image_proc;
pub mod order;

pub use download::{DownloadJob, DownloadOutcome};
pub use image_proc::{downscale, PostProcess};
pub use order::{Card, Cardstock, Details, Face, Order, Source};

use sha1::{Digest, Sha1};

/// The MakePlayingCards family identifies an uploaded image by the SHA-1 of its
/// bytes, upper-cased. We compute it locally to detect already-uploaded images.
pub fn pid(bytes: &[u8]) -> String {
    let digest = Sha1::digest(bytes);
    hex::encode_upper(digest)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to read order file: {0}")]
    Io(#[from] std::io::Error),
    #[error("order XML is malformed: {0}")]
    Xml(#[from] quick_xml::DeError),
    #[error("order is invalid: {0}")]
    Invalid(String),
    #[error("network request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("image processing failed: {0}")]
    Image(#[from] image::ImageError),
}

pub type Result<T> = std::result::Result<T, Error>;
