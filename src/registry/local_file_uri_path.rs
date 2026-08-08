// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Decoding of raw `file:` URI paths into local canonical path text.

use qubit_fs::FsError;
use qubit_fs::Path;
use qubit_fs::Uri;
use qubit_spi::error::ProviderFailure;

use super::local_file_system_provider::invalid_path;

/// Decodes a raw URI path into a canonical local logical path.
///
/// URI percent escapes represent native path bytes, whereas local logical path
/// components use canonical escaped-byte text. Decoding each URI component
/// independently prevents an encoded separator from changing path structure.
///
/// # Parameters
///
/// - `raw`: Raw URI path text, including slash separators.
///
/// # Returns
///
/// A canonical logical path preserving native bytes as uppercase escapes.
///
/// # Errors
///
/// Returns an invalid-configuration failure for malformed percent escapes,
/// NUL bytes, encoded separators, or canonical text rejected by `Path`.
pub(super) fn decode(raw: &str) -> Result<Path, ProviderFailure<FsError>> {
    let mut canonical = String::with_capacity(raw.len());
    for (index, component) in raw.split('/').enumerate() {
        if index > 0 {
            canonical.push('/');
        }
        canonical.push_str(&decode_component(component)?);
    }
    Path::parse(&canonical)
        .map_err(|_| invalid_path("local file URI path is invalid"))
}

/// Re-encodes a canonical logical path as the unique absolute `file:` URI
/// spelling used by registry resolutions.
pub(super) fn canonical_uri(
    path: &Path,
) -> Result<Uri, ProviderFailure<FsError>> {
    let text = path.as_str();
    let mut encoded = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        if text.as_bytes()[index] == b'%'
            && index + 2 < text.len()
            && hex_value(text.as_bytes()[index + 1]).is_some()
            && hex_value(text.as_bytes()[index + 2]).is_some()
        {
            encoded.push('%');
            encoded.push(char::from(text.as_bytes()[index + 1]));
            encoded.push(char::from(text.as_bytes()[index + 2]));
            index += 3;
            continue;
        }
        let scalar = text[index..]
            .chars()
            .next()
            .expect("path byte index must start a scalar");
        if scalar == '/' || is_uri_pchar(scalar) {
            encoded.push(scalar);
        } else {
            for byte in scalar.to_string().as_bytes() {
                push_uri_escaped_byte(&mut encoded, *byte);
            }
        }
        index += scalar.len_utf8();
    }
    Uri::parse(&format!("file://{encoded}")).map_err(|_| {
        invalid_path("local file URI path cannot be canonicalized")
    })
}

/// Returns whether a scalar can appear unescaped in a URI path segment.
fn is_uri_pchar(scalar: char) -> bool {
    scalar.is_ascii_alphanumeric()
        || matches!(
            scalar,
            '-' | '.'
                | '_'
                | '~'
                | '!'
                | '$'
                | '&'
                | '\''
                | '('
                | ')'
                | '*'
                | '+'
                | ','
                | ';'
                | ':'
                | '@'
        )
}

/// Decodes one URI segment and encodes it in the local canonical text form.
///
/// # Parameters
///
/// - `component`: Raw URI segment without slash separators.
///
/// # Returns
///
/// Canonical escaped-byte text for the decoded native bytes.
///
/// # Errors
///
/// Returns an invalid-path failure for malformed percent escapes, NUL bytes,
/// or bytes decoding to a native path separator.
fn decode_component(
    component: &str,
) -> Result<String, ProviderFailure<FsError>> {
    let canonical = canonicalize_uri_bytes(&decode_uri_bytes(component)?);
    let bytes = canonical.as_bytes();
    if bytes.contains(&b'/') || cfg!(windows) && bytes.contains(&b'\\') {
        return Err(invalid_path(
            "local file URI path must not encode a path separator",
        ));
    }
    Ok(canonical)
}

/// Strictly percent-decodes a URI component without treating `+` as a space.
fn decode_uri_bytes(
    component: &str,
) -> Result<Vec<u8>, ProviderFailure<FsError>> {
    let bytes = component.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let high = bytes
            .get(index + 1)
            .copied()
            .and_then(hex_value)
            .ok_or_else(|| {
                invalid_path(
                    "local file URI path contains an invalid encoded component",
                )
            })?;
        let low = bytes
            .get(index + 2)
            .copied()
            .and_then(hex_value)
            .ok_or_else(|| {
                invalid_path(
                    "local file URI path contains an invalid encoded component",
                )
            })?;
        decoded.push((high << 4) | low);
        index += 3;
    }
    if decoded.contains(&0) {
        return Err(invalid_path(
            "local file URI path contains an invalid encoded component",
        ));
    }
    Ok(decoded)
}

/// Converts one ASCII hexadecimal digit to its numeric value.
const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Canonicalizes URI bytes without first constructing a native path value.
fn canonicalize_uri_bytes(bytes: &[u8]) -> String {
    let mut canonical = String::with_capacity(bytes.len());
    let mut remaining = bytes;
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                push_uri_scalars(&mut canonical, valid);
                break;
            }
            Err(error) => {
                let valid_end = error.valid_up_to();
                let valid = std::str::from_utf8(&remaining[..valid_end])
                    .expect("valid UTF-8 prefix must decode");
                push_uri_scalars(&mut canonical, valid);
                let invalid_len = error.error_len().unwrap_or(1);
                for byte in &remaining[valid_end..valid_end + invalid_len] {
                    push_uri_escaped_byte(&mut canonical, *byte);
                }
                remaining = &remaining[valid_end + invalid_len..];
            }
        }
    }
    canonical
}

/// Appends UTF-8 scalars using local canonical escaped-byte text.
fn push_uri_scalars(canonical: &mut String, text: &str) {
    for scalar in text.chars() {
        if scalar == '%' || scalar.is_control() {
            for byte in scalar.to_string().bytes() {
                push_uri_escaped_byte(canonical, byte);
            }
        } else {
            canonical.push(scalar);
        }
    }
}

/// Appends one uppercase percent escape.
fn push_uri_escaped_byte(canonical: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    canonical.push('%');
    canonical.push(char::from(HEX[usize::from(byte >> 4)]));
    canonical.push(char::from(HEX[usize::from(byte & 0x0F)]));
}
