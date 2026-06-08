use std::collections::BTreeMap;

pub fn headers_to_record(headers: &reqwest::header::HeaderMap) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for (key, value) in headers {
        if let Ok(value) = value.to_str() {
            result.insert(key.as_str().to_string(), value.to_string());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    #[test]
    fn converts_headers_to_record() {
        let mut headers = HeaderMap::new();
        headers.insert("x-test", HeaderValue::from_static("value"));
        assert_eq!(
            headers_to_record(&headers)
                .get("x-test")
                .map(String::as_str),
            Some("value")
        );
    }
}
