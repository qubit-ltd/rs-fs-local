// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Decoding of raw `file:` URI paths into local canonical path text.

use qubit_fs::error::FsError;
use qubit_fs::path::Path;
use qubit_fs::path::Uri;
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
    Path::parse(&canonical).map_err(|_| invalid_path("local file URI path is invalid"))
}

/// Re-encodes a canonical logical path as the unique absolute `file:` URI
/// spelling used by registry resolutions.
///
/// # Parameters
///
/// - `path`: Canonical absolute logical path to encode.
///
/// # Returns
///
/// The normalized `file:` URI whose path segment round-trips through
/// [`decode`].
///
/// # Errors
///
/// Returns an invalid-configuration failure when the path cannot be encoded
/// or parsed as a canonical registry URI.
pub(super) fn canonical_uri(path: &Path) -> Result<Uri, ProviderFailure<FsError>> {
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
    Uri::parse(&format!("file://{encoded}")).map_err(|_| invalid_path("local file URI path cannot be canonicalized"))
}

/// Returns whether a scalar can appear unescaped in a URI path segment.
fn is_uri_pchar(scalar: char) -> bool {
    scalar.is_ascii_alphanumeric()
        || matches!(
            scalar,
            '-' | '.' | '_' | '~' | '!' | '$' | '&' | '\'' | '(' | ')' | '*' | '+' | ',' | ';' | ':' | '@'
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
fn decode_component(component: &str) -> Result<String, ProviderFailure<FsError>> {
    let canonical = canonicalize_uri_bytes(&decode_uri_bytes(component)?);
    let bytes = canonical.as_bytes();
    if bytes.contains(&b'/') || cfg!(windows) && bytes.contains(&b'\\') {
        return Err(invalid_path("local file URI path must not encode a path separator"));
    }
    Ok(canonical)
}

/// Strictly percent-decodes a URI component without treating `+` as a space.
///
/// # Parameters
///
/// - `component`: Raw URI segment without slash separators.
///
/// # Returns
///
/// Decoded native path bytes for the segment.
///
/// # Errors
///
/// Returns an invalid-path failure for malformed escapes or embedded NUL bytes.
fn decode_uri_bytes(component: &str) -> Result<Vec<u8>, ProviderFailure<FsError>> {
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
            .ok_or_else(|| invalid_path("local file URI path contains an invalid encoded component"))?;
        let low = bytes
            .get(index + 2)
            .copied()
            .and_then(hex_value)
            .ok_or_else(|| invalid_path("local file URI path contains an invalid encoded component"))?;
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
///
/// # Parameters
///
/// - `byte`: ASCII digit or `a`–`f` / `A`–`F` byte.
///
/// # Returns
///
/// `Some(n)` for a valid digit or `None` for any other byte.
const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Canonicalizes URI bytes without first constructing a native path value.
///
/// # Parameters
///
/// - `bytes`: Decoded segment bytes to encode as canonical escaped text.
///
/// # Returns
///
/// Uppercase percent escapes for non-UTF-8 bytes and controls; UTF-8 scalars
/// are copied when they do not require escaping.
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
                let valid = std::str::from_utf8(&remaining[..valid_end]).expect("valid UTF-8 prefix must decode");
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
///
/// # Parameters
///
/// - `canonical`: Destination buffer receiving canonical path text.
/// - `text`: Valid UTF-8 prefix to append.
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
///
/// # Parameters
///
/// - `canonical`: Destination buffer receiving the escape sequence.
/// - `byte`: Raw byte to encode as `%XX`.
fn push_uri_escaped_byte(canonical: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    canonical.push('%');
    canonical.push(char::from(HEX[usize::from(byte >> 4)]));
    canonical.push(char::from(HEX[usize::from(byte & 0x0F)]));
}

#[cfg(test)]
mod tests {
    use proptest::char::range;
    use proptest::collection;
    use proptest::prop_assert;
    use proptest::prop_assert_eq;
    use proptest::prop_oneof;
    use proptest::proptest;
    use proptest::sample::select;
    use proptest::strategy::Just;
    use proptest::strategy::Strategy;
    use qubit_fs::error::FsErrorKind;
    use qubit_fs::path::Path;
    use qubit_spi::error::ProviderFailureKind;

    use super::canonical_uri;
    use super::decode;

    /// Generates safe native path segments with bounded canonical text.
    fn path_segments() -> impl Strategy<Value = Vec<String>> {
        let scalar = prop_oneof![
            range('a', 'z'),
            range('A', 'Z'),
            range('0', '9'),
            Just(' '),
            Just('%'),
            select(vec!['é', '中', '🦀']),
        ];
        collection::vec(
            collection::vec(scalar, 1..=32).prop_map(|scalars| scalars.into_iter().collect()),
            0..=8,
        )
    }

    /// Converts safe native segments into canonical logical path text.
    fn logical_path(segments: &[String]) -> Path {
        let components = segments
            .iter()
            .map(|segment| segment.replace('%', "%25"))
            .collect::<Vec<_>>();
        Path::parse(&format!("/{}", components.join("/"))).expect("generated canonical path must parse")
    }

    proptest! {
        #[test]
        fn test_decode_canonical_uri_round_trips_safe_paths(
            segments in path_segments(),
        ) {
            let path = logical_path(&segments);
            prop_assert!(path.as_str().len() <= 4096);
            let uri = canonical_uri(&path)
                .expect("generated path must have a canonical URI");
            prop_assert!(uri.path().len() <= 4096);

            prop_assert_eq!(
                decode(uri.path()).expect("canonical URI path must decode"),
                path,
            );
        }

        #[test]
        fn test_canonical_uri_normalizes_decoded_safe_paths(
            segments in path_segments(),
        ) {
            let path = logical_path(&segments);
            prop_assert!(path.as_str().len() <= 4096);
            let expected = canonical_uri(&path)
                .expect("generated path must have a canonical URI");
            prop_assert!(expected.path().len() <= 4096);
            let decoded = decode(expected.path())
                .expect("canonical URI path must decode");

            prop_assert_eq!(
                canonical_uri(&decoded)
                    .expect("decoded path must have a canonical URI"),
                expected,
            );
        }
    }

    /// Malformed escapes and encoded native separators fail without panics.
    #[test]
    fn test_decode_rejects_malformed_and_unsafe_encoded_components() {
        #[cfg(not(windows))]
        let raw_paths = ["/%", "/%0", "/%GG", "/%00", "/%2F"];
        #[cfg(windows)]
        let raw_paths = ["/%", "/%0", "/%GG", "/%00", "/%2F", "/%5C"];

        for raw in raw_paths {
            let failure = decode(raw).expect_err("malformed or unsafe URI path must be rejected");
            assert_eq!(
                ProviderFailureKind::InvalidConfiguration,
                failure.kind(),
                "unexpected provider failure kind for {raw}",
            );
            assert_eq!(
                FsErrorKind::InvalidPath,
                failure.error().kind(),
                "unexpected filesystem error kind for {raw}",
            );
        }
    }
}
