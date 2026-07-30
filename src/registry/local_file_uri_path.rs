// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Decoding of raw `file:` URI paths into local canonical path text.

use qubit_fs::Path;

use super::local_file_system_provider::invalid_path;

/// Decodes a raw URI path into a canonical local logical path.
///
/// URI percent escapes represent native path bytes, whereas local logical path
/// components use canonical escaped-byte text. Decoding each URI component
/// independently prevents an encoded separator from changing path structure.
pub(super) fn decode(
    raw: &str,
) -> Result<Path, qubit_spi::error::ProviderFailure<qubit_fs::FsError>> {
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

/// Decodes one URI segment and encodes it in the local canonical text form.
fn decode_component(
    component: &str,
) -> Result<String, qubit_spi::error::ProviderFailure<qubit_fs::FsError>> {
    let bytes = percent_decode(component)?;
    if bytes.contains(&0) {
        return Err(invalid_path("local file URI path must not contain NUL"));
    }
    if bytes.contains(&b'/') || bytes.contains(&b'\\') {
        return Err(invalid_path(
            "local file URI path must not encode a path separator",
        ));
    }
    Ok(canonicalize_bytes(&bytes))
}

/// Strictly percent-decodes one URI path segment without treating `+` as a
/// space.
fn percent_decode(
    component: &str,
) -> Result<Vec<u8>, qubit_spi::error::ProviderFailure<qubit_fs::FsError>> {
    let bytes = component.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let Some(high) = bytes.get(index + 1).copied() else {
            return Err(invalid_path(
                "local file URI contains an invalid percent escape",
            ));
        };
        let Some(low) = bytes.get(index + 2).copied() else {
            return Err(invalid_path(
                "local file URI contains an invalid percent escape",
            ));
        };
        let (Some(high), Some(low)) = (hex_value(high), hex_value(low)) else {
            return Err(invalid_path(
                "local file URI contains an invalid percent escape",
            ));
        };
        decoded.push((high << 4) | low);
        index += 3;
    }
    Ok(decoded)
}

/// Converts one ASCII hexadecimal digit to its numeric value.
fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

/// Converts decoded native bytes to local canonical escaped-byte text.
fn canonicalize_bytes(bytes: &[u8]) -> String {
    let mut canonical = String::with_capacity(bytes.len());
    let mut remaining = bytes;
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                push_scalars(&mut canonical, valid);
                break;
            }
            Err(error) => {
                let valid_end = error.valid_up_to();
                let valid = std::str::from_utf8(&remaining[..valid_end])
                    .expect("valid UTF-8 prefix must decode");
                push_scalars(&mut canonical, valid);
                let invalid_len = error.error_len().unwrap_or(1);
                for byte in &remaining[valid_end..valid_end + invalid_len] {
                    push_escaped_byte(&mut canonical, *byte);
                }
                remaining = &remaining[valid_end + invalid_len..];
            }
        }
    }
    canonical
}

/// Appends UTF-8 scalars using the local canonical escaped-byte policy.
fn push_scalars(canonical: &mut String, text: &str) {
    for scalar in text.chars() {
        if scalar == '%' || scalar.is_control() {
            for byte in scalar.to_string().bytes() {
                push_escaped_byte(canonical, byte);
            }
        } else {
            canonical.push(scalar);
        }
    }
}

/// Appends one uppercase percent escape.
fn push_escaped_byte(canonical: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    canonical.push('%');
    canonical.push(char::from(HEX[usize::from(byte >> 4)]));
    canonical.push(char::from(HEX[usize::from(byte & 0x0F)]));
}
