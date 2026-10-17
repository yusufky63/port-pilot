//! Just enough HTTP/1.1 to talk to the Docker Engine API over its named pipe: one request per
//! connection, `Connection: close`, plain or chunked bodies.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

pub fn request(method: &str, path: &str) -> Vec<u8> {
    format!(
        "{method} {path} HTTP/1.1\r\nHost: docker\r\nUser-Agent: port-pilot\r\nAccept: application/json\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
    .into_bytes()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn decode_chunked(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = find(data, b"\r\n").ok_or("truncated chunk size")?;
        let size_text = std::str::from_utf8(&data[..line_end]).map_err(|_| "bad chunk size")?;
        let size_text = size_text.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| format!("bad chunk size: {size_text}"))?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size {
            return Err("truncated chunk".into());
        }
        out.extend_from_slice(&data[..size]);
        data = data.get(size + 2..).unwrap_or(&[]);
    }
}

/// Parses a complete response read until the server closed the connection.
pub fn parse_response(raw: &[u8]) -> Result<Response, String> {
    let head_end = find(raw, b"\r\n\r\n").ok_or("no header terminator")?;
    let head = std::str::from_utf8(&raw[..head_end]).map_err(|_| "header is not UTF-8")?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or("empty response")?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| format!("bad status line: {status_line}"))?;
    let mut chunked = false;
    let mut length: Option<usize> = None;
    for line in lines {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
        if k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked") {
            chunked = true;
        } else if k == "content-length" {
            length = v.parse().ok();
        }
    }
    let body = &raw[head_end + 4..];
    let body = if chunked {
        decode_chunked(body)?
    } else if let Some(n) = length {
        body.get(..n).ok_or("truncated body")?.to_vec()
    } else {
        body.to_vec()
    };
    Ok(Response { status, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_length_body() {
        let raw =
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n[]";
        assert_eq!(
            parse_response(raw),
            Ok(Response {
                status: 200,
                body: b"[]".to_vec()
            })
        );
    }

    #[test]
    fn chunked_body() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n[{\"a\r\n5;ext=1\r\n\":1}]\r\n0\r\n\r\n";
        assert_eq!(parse_response(raw).unwrap().body, b"[{\"a\":1}]".to_vec());
    }

    #[test]
    fn no_content_and_errors() {
        assert_eq!(
            parse_response(b"HTTP/1.1 204 No Content\r\n\r\n")
                .unwrap()
                .status,
            204
        );
        assert!(parse_response(b"garbage").is_err());
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nshort").is_err());
        assert!(
            parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n").is_err()
        );
    }

    #[test]
    fn request_shape() {
        let r = String::from_utf8(request("GET", "/containers/json")).unwrap();
        assert!(r.starts_with("GET /containers/json HTTP/1.1\r\n"));
        assert!(r.ends_with("\r\n\r\n"));
    }
}
