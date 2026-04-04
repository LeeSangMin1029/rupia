use crate::types::{ParseError, ParseResult};

const MAX_DEPTH: usize = 512;
const MAX_INPUT_SIZE: usize = 16 * 1024 * 1024;

pub fn parse(input: &str) -> ParseResult<serde_json::Value> {
    if input.len() > MAX_INPUT_SIZE {
        return ParseResult::Failure {
            data: None,
            input: input.to_owned(),
            errors: vec![ParseError {
                path: "$input".into(),
                expected: "input within 16MB".into(),
                description: Some(format!("input size {} exceeds limit", input.len())),
            }],
        };
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(input) {
        return ParseResult::Success(v);
    }
    iterate(input)
}

fn iterate(input: &str) -> ParseResult<serde_json::Value> {
    let json_source = extract_markdown_code_block(input).unwrap_or(input);
    let trimmed = json_source.trim();
    if trimmed.is_empty() {
        return ParseResult::Failure {
            data: None,
            input: input.to_owned(),
            errors: vec![ParseError {
                path: "$input".into(),
                expected: "JSON value".into(),
                description: Some("empty input".into()),
            }],
        };
    }
    if starts_with_primitive(trimmed) {
        let mut errors = Vec::new();
        let mut parser = LenientParser::new(json_source, &mut errors);
        let data = parser.parse();
        return if errors.is_empty() {
            ParseResult::Success(data)
        } else {
            ParseResult::Failure {
                data: Some(data),
                input: input.to_owned(),
                errors,
            }
        };
    }
    let json_start = find_json_start(json_source);
    if json_start.is_none() {
        let skipped = skip_comments_and_whitespace(json_source);
        if !skipped.is_empty() && starts_with_primitive(skipped) {
            let mut errors = Vec::new();
            let mut parser = LenientParser::new(json_source, &mut errors);
            let data = parser.parse();
            return if errors.is_empty() {
                ParseResult::Success(data)
            } else {
                ParseResult::Failure {
                    data: Some(data),
                    input: input.to_owned(),
                    errors,
                }
            };
        }
        return ParseResult::Failure {
            data: None,
            input: input.to_owned(),
            errors: vec![ParseError {
                path: "$input".into(),
                expected: "JSON value".into(),
                description: Some(json_source.to_owned()),
            }],
        };
    }
    let json_input = &json_source[json_start.unwrap()..];
    let mut errors = Vec::new();
    let mut parser = LenientParser::new(json_input, &mut errors);
    let data = parser.parse();
    if errors.is_empty() {
        ParseResult::Success(data)
    } else {
        ParseResult::Failure {
            data: Some(data),
            input: input.to_owned(),
            errors,
        }
    }
}

fn extract_markdown_code_block(input: &str) -> Option<&str> {
    let start = input.find("```json")?;
    let trimmed = input.trim_start();
    if !trimmed.is_empty() {
        let first = trimmed.as_bytes()[0];
        if first == b'{' || first == b'[' || first == b'"' {
            return None;
        }
    }
    let after_marker = start + 7;
    let content_start = input[after_marker..]
        .find('\n')
        .map(|i| after_marker + i + 1)?;
    let end = input[content_start..].find("```");
    match end {
        Some(e) => Some(&input[content_start..content_start + e]),
        None => Some(&input[content_start..]),
    }
}

fn find_json_start(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut pos = 0;
    while pos < len {
        let ch = bytes[pos];
        if ch == b'{' || ch == b'[' {
            return Some(pos);
        }
        if ch == b'/' && pos + 1 < len && bytes[pos + 1] == b'/' {
            pos += 2;
            while pos < len && bytes[pos] != b'\n' && bytes[pos] != b'\r' {
                pos += 1;
            }
            continue;
        }
        if ch == b'/' && pos + 1 < len && bytes[pos + 1] == b'*' {
            pos += 2;
            while pos + 1 < len {
                if bytes[pos] == b'*' && bytes[pos + 1] == b'/' {
                    pos += 2;
                    break;
                }
                pos += 1;
            }
            if pos + 1 >= len {
                pos = len;
            }
            continue;
        }
        if ch == b'"' {
            pos += 1;
            while pos < len {
                if bytes[pos] == b'\\' {
                    pos += 2;
                    continue;
                }
                if bytes[pos] == b'"' {
                    pos += 1;
                    break;
                }
                pos += 1;
            }
            continue;
        }
        pos += 1;
    }
    None
}

fn skip_comments_and_whitespace(input: &str) -> &str {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut pos = 0;
    while pos < len {
        let ch = bytes[pos];
        if ch == b' ' || ch == b'\t' || ch == b'\n' || ch == b'\r' {
            pos += 1;
            continue;
        }
        if ch == b'/' && pos + 1 < len && bytes[pos + 1] == b'/' {
            pos += 2;
            while pos < len && bytes[pos] != b'\n' && bytes[pos] != b'\r' {
                pos += 1;
            }
            continue;
        }
        if ch == b'/' && pos + 1 < len && bytes[pos + 1] == b'*' {
            pos += 2;
            while pos + 1 < len {
                if bytes[pos] == b'*' && bytes[pos + 1] == b'/' {
                    pos += 2;
                    break;
                }
                pos += 1;
            }
            if pos + 1 >= len {
                pos = len;
            }
            continue;
        }
        break;
    }
    &input[pos..]
}

fn starts_with_primitive(input: &str) -> bool {
    if input.is_empty() {
        return false;
    }
    let first = input.as_bytes()[0];
    if first == b'"' {
        return true;
    }
    if first.is_ascii_digit() || first == b'-' {
        return true;
    }
    if input.starts_with("true") || input.starts_with("false") || input.starts_with("null") {
        return true;
    }
    if "true".starts_with(input) || "false".starts_with(input) {
        return true;
    }
    if "null".starts_with(input) && input.len() >= 2 {
        return true;
    }
    let lower = input.to_ascii_lowercase();
    matches!(lower.as_str(), "yes" | "y" | "on" | "no" | "off")
}

struct LenientParser<'a> {
    input: &'a [u8],
    pos: usize,
    depth: usize,
    errors: &'a mut Vec<ParseError>,
}

impl<'a> LenientParser<'a> {
    fn new(input: &'a str, errors: &'a mut Vec<ParseError>) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
            depth: 0,
            errors,
        }
    }

    fn parse(&mut self) -> serde_json::Value {
        self.skip_whitespace();
        if self.pos >= self.input.len() {
            return serde_json::Value::Null;
        }
        self.parse_value("$input")
    }

    fn parse_value(&mut self, path: &str) -> serde_json::Value {
        self.skip_whitespace();
        if self.pos >= self.input.len() {
            return serde_json::Value::Null;
        }
        if self.depth >= MAX_DEPTH {
            self.errors.push(ParseError {
                path: path.to_owned(),
                expected: "value (max depth exceeded)".into(),
                description: None,
            });
            return serde_json::Value::Null;
        }
        let ch = self.input[self.pos];
        match ch {
            b'{' => self.parse_object(path),
            b'[' => self.parse_array(path),
            b'"' => serde_json::Value::String(self.parse_string()),
            b'-' | b'0'..=b'9' => self.parse_number(),
            b'}' | b']' | b',' => serde_json::Value::Null,
            _ if Self::is_ident_start(ch) => self.parse_keyword_or_ident(path),
            _ => {
                self.errors.push(ParseError {
                    path: path.to_owned(),
                    expected: "JSON value".into(),
                    description: Some(self.error_context()),
                });
                self.pos += 1;
                serde_json::Value::Null
            }
        }
    }

    fn parse_object(&mut self, path: &str) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        self.pos += 1;
        self.depth += 1;
        self.skip_whitespace();
        while self.pos < self.input.len() {
            self.skip_whitespace();
            if self.pos >= self.input.len() || self.input[self.pos] == b'}' {
                if self.pos < self.input.len() {
                    self.pos += 1;
                }
                self.depth -= 1;
                return serde_json::Value::Object(map);
            }
            if self.input[self.pos] == b',' {
                self.pos += 1;
                self.skip_whitespace();
                continue;
            }
            let key = if self.input[self.pos] == b'"' {
                self.parse_string()
            } else if Self::is_ident_start(self.input[self.pos]) {
                self.parse_identifier()
            } else {
                self.errors.push(ParseError {
                    path: path.to_owned(),
                    expected: "string key".into(),
                    description: Some(format!("unexpected '{}'", self.input[self.pos] as char)),
                });
                self.depth -= 1;
                return serde_json::Value::Object(map);
            };
            self.skip_whitespace();
            if self.pos >= self.input.len() {
                self.depth -= 1;
                return serde_json::Value::Object(map);
            }
            if self.input[self.pos] != b':' {
                self.errors.push(ParseError {
                    path: format!("{path}.{key}"),
                    expected: "':'".into(),
                    description: Some(format!("got '{}'", self.input[self.pos] as char)),
                });
                self.depth -= 1;
                return serde_json::Value::Object(map);
            }
            self.pos += 1;
            self.skip_whitespace();
            if self.pos >= self.input.len() {
                self.depth -= 1;
                return serde_json::Value::Object(map);
            }
            let value = self.parse_value(&format!("{path}.{key}"));
            map.insert(key, value);
            self.skip_whitespace();
            if self.pos < self.input.len() && self.input[self.pos] == b',' {
                self.pos += 1;
            }
        }
        self.depth -= 1;
        serde_json::Value::Object(map)
    }

    fn parse_array(&mut self, path: &str) -> serde_json::Value {
        let mut arr = Vec::new();
        self.pos += 1;
        self.depth += 1;
        self.skip_whitespace();
        let mut index = 0usize;
        while self.pos < self.input.len() {
            self.skip_whitespace();
            if self.pos >= self.input.len() || self.input[self.pos] == b']' {
                if self.pos < self.input.len() {
                    self.pos += 1;
                }
                self.depth -= 1;
                return serde_json::Value::Array(arr);
            }
            if self.input[self.pos] == b',' {
                self.pos += 1;
                self.skip_whitespace();
                continue;
            }
            let prev_pos = self.pos;
            let value = self.parse_value(&format!("{path}[{index}]"));
            if self.pos == prev_pos && self.pos < self.input.len() {
                self.pos += 1;
                continue;
            }
            arr.push(value);
            index += 1;
            self.skip_whitespace();
            if self.pos < self.input.len() && self.input[self.pos] == b',' {
                self.pos += 1;
            }
        }
        self.depth -= 1;
        serde_json::Value::Array(arr)
    }

    fn parse_string(&mut self) -> String {
        self.pos += 1;
        let mut result = String::new();
        let mut escaped = false;
        while self.pos < self.input.len() {
            let ch = self.input[self.pos];
            if escaped {
                match ch {
                    b'"' => result.push('"'),
                    b'\\' => result.push('\\'),
                    b'/' => result.push('/'),
                    b'b' => result.push('\u{0008}'),
                    b'f' => result.push('\u{000C}'),
                    b'n' => result.push('\n'),
                    b'r' => result.push('\r'),
                    b't' => result.push('\t'),
                    b'u' => {
                        if self.pos + 4 < self.input.len() {
                            let hex = &self.input[self.pos + 1..self.pos + 5];
                            if let Ok(hex_str) = std::str::from_utf8(hex) {
                                if let Ok(code) = u16::from_str_radix(hex_str, 16) {
                                    self.pos += 4;
                                    if (0xD800..=0xDBFF).contains(&code)
                                        && self.pos + 6 < self.input.len()
                                        && self.input[self.pos + 1] == b'\\'
                                        && self.input[self.pos + 2] == b'u'
                                    {
                                        let low_hex = &self.input[self.pos + 3..self.pos + 7];
                                        if let Ok(low_str) = std::str::from_utf8(low_hex) {
                                            if let Ok(low) = u16::from_str_radix(low_str, 16) {
                                                if (0xDC00..=0xDFFF).contains(&low) {
                                                    if let Some(c) = char::from_u32(
                                                        u32::from(code - 0xD800) * 0x400
                                                            + u32::from(low - 0xDC00)
                                                            + 0x10000,
                                                    ) {
                                                        result.push(c);
                                                        self.pos += 6;
                                                        escaped = false;
                                                        self.pos += 1;
                                                        continue;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    if let Some(c) = char::from_u32(u32::from(code)) {
                                        result.push(c);
                                    }
                                } else {
                                    result.push_str("\\u");
                                    result.push_str(hex_str);
                                    self.pos += 4;
                                }
                            }
                        }
                    }
                    _ => result.push(ch as char),
                }
                escaped = false;
                self.pos += 1;
                continue;
            }
            if ch == b'\\' {
                escaped = true;
                self.pos += 1;
                continue;
            }
            if ch == b'"' {
                self.pos += 1;
                return result;
            }
            result.push(ch as char);
            self.pos += 1;
        }
        result
    }

    fn parse_number(&mut self) -> serde_json::Value {
        let start = self.pos;
        if self.pos < self.input.len() && self.input[self.pos] == b'-' {
            self.pos += 1;
        }
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        if self.pos < self.input.len() && self.input[self.pos] == b'.' {
            self.pos += 1;
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        if self.pos < self.input.len()
            && (self.input[self.pos] == b'e' || self.input[self.pos] == b'E')
        {
            self.pos += 1;
            if self.pos < self.input.len()
                && (self.input[self.pos] == b'+' || self.input[self.pos] == b'-')
            {
                self.pos += 1;
            }
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        let num_str = std::str::from_utf8(&self.input[start..self.pos]).unwrap_or("0");
        if let Ok(n) = num_str.parse::<i64>() {
            return serde_json::Value::Number(n.into());
        }
        if let Ok(n) = num_str.parse::<f64>() {
            if let Some(n) = serde_json::Number::from_f64(n) {
                return serde_json::Value::Number(n);
            }
        }
        serde_json::Value::Number(0.into())
    }

    fn parse_keyword_or_ident(&mut self, path: &str) -> serde_json::Value {
        let start = self.pos;
        while self.pos < self.input.len() && Self::is_ident_char(self.input[self.pos]) {
            self.pos += 1;
        }
        let token = std::str::from_utf8(&self.input[start..self.pos]).unwrap_or("");
        match token {
            "true" => return serde_json::Value::Bool(true),
            "false" => return serde_json::Value::Bool(false),
            "null" => return serde_json::Value::Null,
            _ => {}
        }
        let lower = token.to_ascii_lowercase();
        match lower.as_str() {
            "yes" | "y" | "on" => return serde_json::Value::Bool(true),
            "no" | "off" => return serde_json::Value::Bool(false),
            _ => {}
        }
        if "true".starts_with(token) && !token.is_empty() {
            return serde_json::Value::Bool(true);
        }
        if "false".starts_with(token) && !token.is_empty() {
            return serde_json::Value::Bool(false);
        }
        if "null".starts_with(token) && token.len() >= 2 {
            return serde_json::Value::Null;
        }
        self.errors.push(ParseError {
            path: path.to_owned(),
            expected: "JSON value".into(),
            description: Some(format!("unquoted string '{token}'")),
        });
        self.skip_to_recovery();
        serde_json::Value::Null
    }

    fn skip_to_recovery(&mut self) {
        while self.pos < self.input.len() {
            let ch = self.input[self.pos];
            if ch == b',' || ch == b'}' || ch == b']' {
                return;
            }
            self.pos += 1;
        }
    }

    fn parse_identifier(&mut self) -> String {
        let start = self.pos;
        while self.pos < self.input.len() && Self::is_ident_char(self.input[self.pos]) {
            self.pos += 1;
        }
        std::str::from_utf8(&self.input[start..self.pos])
            .unwrap_or("")
            .to_owned()
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() {
            let ch = self.input[self.pos];
            if ch == b' ' || ch == b'\t' || ch == b'\n' || ch == b'\r' {
                self.pos += 1;
                continue;
            }
            if ch == b'/' && self.pos + 1 < self.input.len() && self.input[self.pos + 1] == b'/' {
                self.pos += 2;
                while self.pos < self.input.len()
                    && self.input[self.pos] != b'\n'
                    && self.input[self.pos] != b'\r'
                {
                    self.pos += 1;
                }
                continue;
            }
            if ch == b'/' && self.pos + 1 < self.input.len() && self.input[self.pos + 1] == b'*' {
                self.pos += 2;
                while self.pos + 1 < self.input.len() {
                    if self.input[self.pos] == b'*' && self.input[self.pos + 1] == b'/' {
                        self.pos += 2;
                        break;
                    }
                    self.pos += 1;
                }
                if self.pos + 1 >= self.input.len() {
                    self.pos = self.input.len();
                }
                continue;
            }
            break;
        }
    }

    fn is_ident_start(ch: u8) -> bool {
        ch.is_ascii_alphabetic() || ch == b'_' || ch == b'$'
    }

    fn is_ident_char(ch: u8) -> bool {
        ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'$'
    }

    fn error_context(&self) -> String {
        let start = self.pos.saturating_sub(10);
        let end = (self.pos + 20).min(self.input.len());
        let before = std::str::from_utf8(&self.input[start..self.pos]).unwrap_or("");
        let after = std::str::from_utf8(&self.input[self.pos..end]).unwrap_or("");
        let prefix = if start > 0 { "..." } else { "" };
        let suffix = if end < self.input.len() { "..." } else { "" };
        format!("{prefix}{before}→{after}{suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_json() {
        let r = parse(r#"{"name":"test","age":25}"#);
        assert!(r.is_success());
    }

    #[test]
    fn trailing_comma() {
        let r = parse(r#"{"a": 1, "b": 2, }"#);
        assert!(r.is_success());
        if let ParseResult::Success(v) = r {
            assert_eq!(v["a"], 1);
            assert_eq!(v["b"], 2);
        }
    }

    #[test]
    fn unclosed_brace() {
        let r = parse(r#"{"name": "test", "age": 25"#);
        assert!(r.is_success());
        if let ParseResult::Success(v) = r {
            assert_eq!(v["name"], "test");
            assert_eq!(v["age"], 25);
        }
    }

    #[test]
    fn markdown_code_block() {
        let input = "Here is your JSON:\n```json\n{\"name\": \"test\"}\n```\nDone!";
        let r = parse(input);
        assert!(r.is_success());
        if let ParseResult::Success(v) = r {
            assert_eq!(v["name"], "test");
        }
    }

    #[test]
    fn unquoted_keys() {
        let r = parse(r#"{name: "test", age: 25}"#);
        assert!(r.is_success());
        if let ParseResult::Success(v) = r {
            assert_eq!(v["name"], "test");
            assert_eq!(v["age"], 25);
        }
    }

    #[test]
    fn partial_keywords() {
        let r = parse("tru");
        assert!(r.is_success());
        if let ParseResult::Success(v) = r {
            assert_eq!(v, serde_json::Value::Bool(true));
        }
    }

    #[test]
    fn js_comments() {
        let input = r#"{
            // this is a comment
            "name": "test", /* inline */
            "age": 25
        }"#;
        let r = parse(input);
        assert!(r.is_success());
        if let ParseResult::Success(v) = r {
            assert_eq!(v["name"], "test");
        }
    }

    #[test]
    fn junk_prefix() {
        let input = r#"Sure! Here is the JSON: {"name": "test"}"#;
        let r = parse(input);
        assert!(r.is_success());
        if let ParseResult::Success(v) = r {
            assert_eq!(v["name"], "test");
        }
    }

    #[test]
    fn unclosed_string() {
        let r = parse(r#"{"name": "hello"#);
        assert!(r.is_success());
        if let ParseResult::Success(v) = r {
            assert_eq!(v["name"], "hello");
        }
    }

    #[test]
    fn unicode_escape() {
        let r = parse(r#"{"emoji": "\u0048\u0065\u006C\u006C\u006F"}"#);
        assert!(r.is_success());
        if let ParseResult::Success(v) = r {
            assert_eq!(v["emoji"], "Hello");
        }
    }

    #[test]
    fn empty_input() {
        let r = parse("");
        assert!(!r.is_success());
    }

    #[test]
    fn boolean_strings() {
        assert!(matches!(
            parse("yes"),
            ParseResult::Success(serde_json::Value::Bool(true))
        ));
        assert!(matches!(
            parse("no"),
            ParseResult::Success(serde_json::Value::Bool(false))
        ));
        assert!(matches!(
            parse("on"),
            ParseResult::Success(serde_json::Value::Bool(true))
        ));
        assert!(matches!(
            parse("off"),
            ParseResult::Success(serde_json::Value::Bool(false))
        ));
    }

    #[test]
    fn nested_array() {
        let r = parse(r#"[1, [2, 3], {"a": 4}]"#);
        assert!(r.is_success());
        if let ParseResult::Success(v) = r {
            assert_eq!(v[0], 1);
            assert_eq!(v[1][0], 2);
            assert_eq!(v[2]["a"], 4);
        }
    }
}
