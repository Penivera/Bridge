//! Zero-allocation, bounds-safe TLS SNI (Server Name Indication) and HTTP Host header extractor.
//!
//! Designed for L4 transparent passthrough routers where TLS is not terminated on the proxy.

/// Extracts the Server Name Indication (SNI) hostname from a TLS ClientHello byte buffer.
///
/// Returns `Some(hostname)` in lowercase if a valid TLS ClientHello with an SNI extension is found.
/// Returns `None` if the payload is not a TLS ClientHello, has no SNI, or is malformed.
pub fn parse_tls_sni(buf: &[u8]) -> Option<String> {
    // A TLS record header is at least 5 bytes
    if buf.len() < 5 {
        return None;
    }

    // Byte 0 must be 0x16 (Handshake record)
    if buf[0] != 0x16 {
        return None;
    }

    // Bytes 1..3: Record version (e.g. 0x03, 0x01 or 0x03, 0x03)
    // Bytes 3..5: Record length
    let record_len = u16::from_be_bytes([buf[3], buf[4]]) as usize;
    if record_len < 4 {
        return None;
    }

    // Limit parsing to the record length or buffer length
    let record_end = std::cmp::min(5 + record_len, buf.len());
    let data = &buf[5..record_end];

    // Handshake header:
    // Byte 0: Handshake Type (0x01 == ClientHello)
    // Bytes 1..4: Length (3-byte big-endian integer)
    if data.len() < 4 || data[0] != 0x01 {
        return None;
    }

    let client_hello_len =
        ((data[1] as usize) << 16) | ((data[2] as usize) << 8) | (data[3] as usize);
    let ch_end = std::cmp::min(4 + client_hello_len, data.len());
    let ch_data = &data[4..ch_end];

    // ClientHello body:
    // Bytes 0..2: Client Version (2 bytes)
    // Bytes 2..34: Random (32 bytes)
    let mut offset = 34;
    if ch_data.len() < offset + 1 {
        return None;
    }

    // Session ID length (1 byte)
    let session_id_len = ch_data[offset] as usize;
    offset += 1 + session_id_len;

    // Cipher suites length (2 bytes big-endian)
    if ch_data.len() < offset + 2 {
        return None;
    }
    let cipher_suites_len = u16::from_be_bytes([ch_data[offset], ch_data[offset + 1]]) as usize;
    offset += 2 + cipher_suites_len;

    // Compression methods length (1 byte)
    if ch_data.len() < offset + 1 {
        return None;
    }
    let comp_methods_len = ch_data[offset] as usize;
    offset += 1 + comp_methods_len;

    // Extensions total length (2 bytes big-endian)
    if ch_data.len() < offset + 2 {
        return None;
    }
    let extensions_len = u16::from_be_bytes([ch_data[offset], ch_data[offset + 1]]) as usize;
    offset += 2;

    let ext_end = std::cmp::min(offset + extensions_len, ch_data.len());
    let mut ext_offset = offset;

    // Iterate through TLS extensions
    while ext_offset + 4 <= ext_end {
        let ext_type = u16::from_be_bytes([ch_data[ext_offset], ch_data[ext_offset + 1]]);
        let ext_len =
            u16::from_be_bytes([ch_data[ext_offset + 2], ch_data[ext_offset + 3]]) as usize;
        ext_offset += 4;

        if ext_offset + ext_len > ext_end {
            break;
        }

        let ext_data = &ch_data[ext_offset..ext_offset + ext_len];
        ext_offset += ext_len;

        // Extension 0x0000 is server_name (SNI)
        if ext_type == 0x0000 {
            if ext_data.len() < 2 {
                continue;
            }
            let list_len = u16::from_be_bytes([ext_data[0], ext_data[1]]) as usize;
            let list_end = std::cmp::min(2 + list_len, ext_data.len());
            let mut name_offset = 2;

            while name_offset + 3 <= list_end {
                let name_type = ext_data[name_offset];
                let name_len =
                    u16::from_be_bytes([ext_data[name_offset + 1], ext_data[name_offset + 2]])
                        as usize;
                name_offset += 3;

                if name_offset + name_len > list_end {
                    break;
                }

                // name_type 0x00 is host_name
                if name_type == 0x00 {
                    let host_bytes = &ext_data[name_offset..name_offset + name_len];
                    if let Ok(host_str) = std::str::from_utf8(host_bytes) {
                        return Some(host_str.trim().to_ascii_lowercase());
                    }
                }
                name_offset += name_len;
            }
        }
    }

    None
}

/// Extracts the `Host` header value from a plaintext HTTP/1.x request byte buffer.
///
/// Returns `Some(hostname)` in lowercase (stripping any port suffix) if found.
pub fn parse_http_host(buf: &[u8]) -> Option<String> {
    // Only attempt if it looks like an HTTP method
    let valid_prefix = buf.starts_with(b"GET ")
        || buf.starts_with(b"POST ")
        || buf.starts_with(b"HEAD ")
        || buf.starts_with(b"PUT ")
        || buf.starts_with(b"DELETE ")
        || buf.starts_with(b"OPTIONS ")
        || buf.starts_with(b"PATCH ")
        || buf.starts_with(b"CONNECT ")
        || buf.starts_with(b"TRACE ");

    if !valid_prefix {
        return None;
    }

    let text = std::str::from_utf8(buf).unwrap_or_else(|e| {
        let valid_up_to = e.valid_up_to();
        std::str::from_utf8(&buf[..valid_up_to]).unwrap_or("")
    });

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // End of HTTP headers
            break;
        }

        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("host:") {
            let host_val = trimmed[5..].trim();
            // Handle IPv6 literal "[::1]:8080" vs IPv4/hostname "domain.com:80"
            let host = if host_val.starts_with('[') {
                if let Some(end_bracket) = host_val.find(']') {
                    &host_val[1..end_bracket]
                } else {
                    host_val
                }
            } else if let Some(colon) = host_val.find(':') {
                &host_val[..colon]
            } else {
                host_val
            };

            if !host.is_empty() {
                return Some(host.to_ascii_lowercase());
            }
        }
    }

    None
}

/// Extracts the URI path from the request line of a plaintext HTTP request (e.g. "/foo/bar?baz").
pub fn parse_http_uri_path(buf: &[u8]) -> String {
    let text = std::str::from_utf8(buf).unwrap_or("");
    if let Some(first_line) = text.lines().next() {
        let parts: Vec<&str> = first_line.split_whitespace().collect();
        if parts.len() >= 2 {
            return parts[1].to_string();
        }
    }
    "/".to_string()
}
