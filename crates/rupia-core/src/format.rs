use regex_lite::Regex;
use std::sync::OnceLock;

fn re<'a>(lock: &'a OnceLock<Regex>, pattern: &str) -> &'a Regex {
    lock.get_or_init(|| Regex::new(pattern).unwrap())
}

pub fn validate(s: &str, format: &str) -> bool {
    match format {
        "email" => is_email(s),
        "idn-email" => is_idn_email(s),
        "uri" => is_uri(s),
        "url" => is_url(s),
        "uri-reference" => is_uri_reference(s),
        "uri-template" => is_uri_template(s),
        "iri" | "iri-reference" => is_iri(s),
        "uuid" => is_uuid(s),
        "date-time" | "datetime" => is_date_time(s),
        "date" => is_date(s),
        "time" => is_time(s),
        "duration" => is_duration(s),
        "ipv4" => is_ipv4(s),
        "ipv6" => is_ipv6(s),
        "hostname" => is_hostname(s),
        "idn-hostname" => is_idn_hostname(s),
        "json-pointer" => is_json_pointer(s),
        "relative-json-pointer" => is_relative_json_pointer(s),
        "byte" => is_byte(s),
        "regex" => is_regex(s),
        // "password" and unknown formats always pass (Typia compatible)
        _ => true,
    }
}

fn is_email(s: &str) -> bool {
    static L: OnceLock<Regex> = OnceLock::new();
    re(&L, r"(?i)^[a-z0-9!#$%&'*+/=?^_`{|}~-]+(?:\.[a-z0-9!#$%&'*+/=?^_`{|}~-]+)*@(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$").is_match(s)
}

fn is_idn_email(s: &str) -> bool {
    static L: OnceLock<Regex> = OnceLock::new();
    re(&L, r"(?i)^(([^<>()\[\].,;:\s@]+(\.[^<>()\[\].,;:\s@]+)*)|(.+))@(([^<>()\[\].,;:\s@]+\.)+[^<>()\[\].,;:\s@]{2,})$").is_match(s)
}

fn is_uri(s: &str) -> bool {
    static L: OnceLock<Regex> = OnceLock::new();
    s.contains(':') && re(&L, r"(?i)^[a-z][a-z0-9+\-.]*:.+$").is_match(s)
}

fn is_url(s: &str) -> bool {
    static L: OnceLock<Regex> = OnceLock::new();
    re(&L, r"(?i)^https?://[^\s/$.?#].[^\s]*$").is_match(s)
}

fn is_uri_reference(s: &str) -> bool {
    s.is_empty() || is_uri(s) || s.starts_with('/') || s.starts_with('#') || s.starts_with('?')
}

fn is_uri_template(s: &str) -> bool {
    static L: OnceLock<Regex> = OnceLock::new();
    re(&L, r"(?i)^([^\x00-\x20<>%\\^`|]|%[0-9a-f]{2}|\{[+#./;?&=,!@|]?[a-z0-9_,%]+\})*$").is_match(s)
}

fn is_iri(s: &str) -> bool {
    static L: OnceLock<Regex> = OnceLock::new();
    re(&L, r"^[A-Za-z][\d+\-.A-Za-z]*:[^\x00-\x20<>\\^`|]*$").is_match(s)
}

fn is_uuid(s: &str) -> bool {
    static L: OnceLock<Regex> = OnceLock::new();
    re(&L, r"(?i)^(?:urn:uuid:)?[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$").is_match(s)
}

fn is_date_time(s: &str) -> bool {
    static L: OnceLock<Regex> = OnceLock::new();
    re(&L, r"(?i)^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])[T\s]([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9](\.[0-9]{1,9})?(Z|[+-]([01][0-9]|2[0-3]):[0-5][0-9])$").is_match(s)
}

fn is_date(s: &str) -> bool {
    static L: OnceLock<Regex> = OnceLock::new();
    re(&L, r"^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])$").is_match(s)
}

fn is_time(s: &str) -> bool {
    static L: OnceLock<Regex> = OnceLock::new();
    re(&L, r"(?i)^([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9](\.[0-9]{1,9})?(Z|[+-]([01][0-9]|2[0-3]):[0-5][0-9])$").is_match(s)
}

fn is_duration(s: &str) -> bool {
    if !s.starts_with('P') || s == "P" || s == "PT" {
        return false;
    }
    static L: OnceLock<Regex> = OnceLock::new();
    re(&L, r"^P((\d+Y)?(\d+M)?(\d+D)?(T(\d+H)?(\d+M)?(\d+S)?)?|(\d+W))$").is_match(s)
}

fn is_ipv4(s: &str) -> bool {
    static L: OnceLock<Regex> = OnceLock::new();
    re(&L, r"^(?:(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\.){3}(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)$").is_match(s)
}

fn is_ipv6(s: &str) -> bool {
    static L: OnceLock<Regex> = OnceLock::new();
    re(&L, r"(?i)^((([0-9a-f]{1,4}:){7}([0-9a-f]{1,4}|:))|(([0-9a-f]{1,4}:){6}(:[0-9a-f]{1,4}|((25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3})|:))|(([0-9a-f]{1,4}:){5}(((:[0-9a-f]{1,4}){1,2})|:((25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3})|:))|(([0-9a-f]{1,4}:){4}(((:[0-9a-f]{1,4}){1,3})|((:[0-9a-f]{1,4})?:((25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}))|:))|(([0-9a-f]{1,4}:){3}(((:[0-9a-f]{1,4}){1,4})|((:[0-9a-f]{1,4}){0,2}:((25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}))|:))|(([0-9a-f]{1,4}:){2}(((:[0-9a-f]{1,4}){1,5})|((:[0-9a-f]{1,4}){0,3}:((25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}))|:))|(([0-9a-f]{1,4}:){1}(((:[0-9a-f]{1,4}){1,6})|((:[0-9a-f]{1,4}){0,4}:((25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}))|:))|(:(((:[0-9a-f]{1,4}){1,7})|((:[0-9a-f]{1,4}){0,5}:((25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}))|:)))$").is_match(s)
}

fn is_hostname(s: &str) -> bool {
    let trimmed = s.strip_suffix('.').unwrap_or(s);
    if trimmed.is_empty() || trimmed.len() > 253 {
        return false;
    }
    static L: OnceLock<Regex> = OnceLock::new();
    re(&L, r"(?i)^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[-0-9a-z]{0,61}[0-9a-z])?)*\.?$").is_match(s)
}

fn is_idn_hostname(s: &str) -> bool {
    if s.is_empty() || s.len() > 253 {
        return false;
    }
    s.split('.').all(|label| {
        !label.is_empty() && label.len() <= 63 && !label.starts_with('-') && !label.ends_with('-')
    })
}

fn is_json_pointer(s: &str) -> bool {
    static L: OnceLock<Regex> = OnceLock::new();
    re(&L, r"^(?:/(?:[^~/]|~0|~1)*)*$").is_match(s)
}

fn is_relative_json_pointer(s: &str) -> bool {
    static L: OnceLock<Regex> = OnceLock::new();
    re(&L, r"^(?:0|[1-9][0-9]*)(?:#|(?:/(?:[^~/]|~0|~1)*)*)$").is_match(s)
}

fn is_byte(s: &str) -> bool {
    static L: OnceLock<Regex> = OnceLock::new();
    re(&L, r"^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$").is_match(s)
}

fn is_regex(s: &str) -> bool {
    Regex::new(s).is_ok()
}

pub fn supported_formats() -> &'static [&'static str] {
    &[
        "email", "idn-email", "uri", "url", "uri-reference", "uri-template",
        "iri", "iri-reference", "uuid", "date-time", "date", "time", "duration",
        "ipv4", "ipv6", "hostname", "idn-hostname", "json-pointer",
        "relative-json-pointer", "byte", "regex", "password",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_valid() {
        assert!(validate("test@example.com", "email"));
        assert!(validate("user.name+tag@domain.co.uk", "email"));
        assert!(!validate("not-an-email", "email"));
        assert!(!validate("@missing-local.com", "email"));
    }

    #[test]
    fn uuid_valid() {
        assert!(validate("550e8400-e29b-41d4-a716-446655440000", "uuid"));
        assert!(validate("urn:uuid:550e8400-e29b-41d4-a716-446655440000", "uuid"));
        assert!(!validate("not-a-uuid", "uuid"));
    }

    #[test]
    fn date_time_valid() {
        assert!(validate("2024-01-15T10:30:00Z", "date-time"));
        assert!(validate("2024-01-15T10:30:00+09:00", "date-time"));
        assert!(!validate("2024-01-15", "date-time"));
        assert!(!validate("not-a-date", "date-time"));
    }

    #[test]
    fn date_valid() {
        assert!(validate("2024-01-15", "date"));
        assert!(!validate("2024-13-01", "date"));
    }

    #[test]
    fn time_valid() {
        assert!(validate("10:30:00Z", "time"));
        assert!(!validate("25:00:00Z", "time"));
    }

    #[test]
    fn duration_valid() {
        assert!(validate("P1Y2M3D", "duration"));
        assert!(validate("PT1H30M", "duration"));
        assert!(!validate("P", "duration"));
    }

    #[test]
    fn ipv4_valid() {
        assert!(validate("192.168.1.1", "ipv4"));
        assert!(!validate("256.1.1.1", "ipv4"));
    }

    #[test]
    fn ipv6_valid() {
        assert!(validate("::1", "ipv6"));
        assert!(validate("2001:db8::1", "ipv6"));
        assert!(!validate("not-ipv6", "ipv6"));
    }

    #[test]
    fn hostname_valid() {
        assert!(validate("example.com", "hostname"));
        assert!(!validate("-invalid.com", "hostname"));
    }

    #[test]
    fn uri_valid() {
        assert!(validate("https://example.com/path", "uri"));
        assert!(!validate("not a uri", "uri"));
    }

    #[test]
    fn url_valid() {
        assert!(validate("https://example.com", "url"));
        assert!(!validate("not-a-url", "url"));
    }

    #[test]
    fn json_pointer_valid() {
        assert!(validate("", "json-pointer"));
        assert!(validate("/foo/bar", "json-pointer"));
    }

    #[test]
    fn byte_valid() {
        assert!(validate("SGVsbG8=", "byte"));
        assert!(!validate("not base64!", "byte"));
    }

    #[test]
    fn regex_valid() {
        assert!(validate("^[a-z]+$", "regex"));
        assert!(!validate("[invalid", "regex"));
    }

    #[test]
    fn password_always_true() {
        assert!(validate("anything", "password"));
    }

    #[test]
    fn idn_hostname_valid() {
        assert!(validate("example.com", "idn-hostname"));
        assert!(!validate("", "idn-hostname"));
    }

    #[test]
    fn uri_template_valid() {
        assert!(validate("https://example.com/users/{id}", "uri-template"));
    }

    #[test]
    fn relative_json_pointer_valid() {
        assert!(validate("0/foo", "relative-json-pointer"));
        assert!(validate("1#", "relative-json-pointer"));
    }

    #[test]
    fn iri_valid() {
        assert!(validate("https://example.com", "iri"));
    }

    #[test]
    fn all_22_formats_exist() {
        assert_eq!(supported_formats().len(), 22);
    }
}
