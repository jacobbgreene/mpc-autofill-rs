//! The upstream GLib regression must run optimized: debug builds can mask the
//! invalid immutable C out-pointer that RUSTSEC-2024-0429 describes.
#![cfg(target_os = "linux")]

use glib::prelude::*;

#[test]
fn variant_string_iteration_is_sound_with_optimized_code() {
    let values = ["front", "back", "カード", ""];
    let variant = values.as_slice().to_variant();
    assert_eq!(
        variant.array_iter_str().unwrap().collect::<Vec<_>>(),
        values
    );
    assert_eq!(
        variant.array_iter_str().unwrap().rev().collect::<Vec<_>>(),
        ["", "カード", "back", "front"]
    );
    assert_eq!(variant.array_iter_str().unwrap().last(), Some(""));
    assert_eq!(variant.array_iter_str().unwrap().nth(2), Some("カード"));
    assert_eq!(variant.array_iter_str().unwrap().nth_back(2), Some("back"));

    let mut mixed = variant.array_iter_str().unwrap();
    assert_eq!(mixed.next(), Some("front"));
    assert_eq!(mixed.next_back(), Some(""));
    assert_eq!(mixed.nth(0), Some("back"));
    assert_eq!(mixed.nth_back(0), Some("カード"));
    assert_eq!(mixed.next(), None);
    assert_eq!(mixed.next_back(), None);

    let mut exhausted = variant.array_iter_str().unwrap();
    assert_eq!(exhausted.nth(usize::MAX), None);
    assert_eq!(exhausted.next(), None);
    let mut exhausted_reverse = variant.array_iter_str().unwrap();
    assert_eq!(exhausted_reverse.nth_back(usize::MAX), None);
    assert_eq!(exhausted_reverse.next_back(), None);

    let empty = Vec::<String>::new().to_variant();
    assert_eq!(empty.array_iter_str().unwrap().next(), None);
    assert_eq!(empty.array_iter_str().unwrap().next_back(), None);
    assert_eq!(empty.array_iter_str().unwrap().last(), None);
}
