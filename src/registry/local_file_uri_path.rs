// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Decoding of raw `file:` URI paths into local canonical path text.

use qubit_fs::Path;
use qubit_local_files::LocalPathCodec;

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
/// or bytes decoding to a slash or backslash.
fn decode_component(
    component: &str,
) -> Result<String, qubit_spi::error::ProviderFailure<qubit_fs::FsError>> {
    let canonical =
        LocalPathCodec::decode_uri_component(component).map_err(|_| {
            invalid_path(
                "local file URI path contains an invalid encoded component",
            )
        })?;
    let bytes = canonical.as_bytes();
    if bytes.contains(&b'/') || bytes.contains(&b'\\') {
        return Err(invalid_path(
            "local file URI path must not encode a path separator",
        ));
    }
    Ok(canonical)
}
