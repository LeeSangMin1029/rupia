use regex_lite::Regex;
use std::sync::OnceLock;

#[expect(
    clippy::too_many_lines,
    reason = "22 format dispatch, splitting reduces readability"
)]
pub fn validate(s: &str, format: &str) -> bool {
    match format {
        "email" => re_match(
            s,
            &RE_EMAIL,
            r"(?i)^[a-z0-9!#$%&'*+/=?^_`{|}~-]+(?:\.[a-z0-9!#$%&'*+/=?^_`{|}~-]+)*@(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$",
        ),
        "idn-email" => re_match(
            s,
            &RE_IDN_EMAIL,
            r"(?i)^(([^<>()\[\].,;:\s@]+(\.[^<>()\[\].,;:\s@]+)*)|(.+))@(([^<>()\[\].,;:\s@]+\.)+[^<>()\[\].,;:\s@]{2,})$",
        ),
        "uri" => s.contains(':') && re_match(s, &RE_URI, r"(?i)^[a-z][a-z0-9+\-.]*:.+$"),
        "url" => re_match(s, &RE_URL, r"(?i)^https?://[^\s/$.?#].[^\s]*$"),
        "uri-reference" => {
            s.is_empty()
                || validate(s, "uri")
                || s.starts_with('/')
                || s.starts_with('#')
                || s.starts_with('?')
        }
        "uri-template" => re_match(
            s,
            &RE_URI_TPL,
            r"(?i)^([^\x00-\x20<>%\\^`|]|%[0-9a-f]{2}|\{[+#./;?&=,!@|]?[a-z0-9_,%]+\})*$",
        ),
        "iri" | "iri-reference" => re_match(
            s,
            &RE_IRI,
            r"^[A-Za-z][\d+\-.A-Za-z]*:[^\x00-\x20<>\\^`|]*$",
        ),
        "uuid" => re_match(
            s,
            &RE_UUID,
            r"(?i)^(?:urn:uuid:)?[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$",
        ),
        "date-time" | "datetime" => re_match(
            s,
            &RE_DT,
            r"(?i)^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])[T\s]([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9](\.[0-9]{1,9})?(Z|[+-]([01][0-9]|2[0-3]):[0-5][0-9])$",
        ),
        "date" => re_match(
            s,
            &RE_DATE,
            r"^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])$",
        ),
        "time" => re_match(
            s,
            &RE_TIME,
            r"(?i)^([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9](\.[0-9]{1,9})?(Z|[+-]([01][0-9]|2[0-3]):[0-5][0-9])$",
        ),
        "duration" => {
            s.starts_with('P')
                && s != "P"
                && s != "PT"
                && re_match(
                    s,
                    &RE_DUR,
                    r"^P((\d+Y)?(\d+M)?(\d+D)?(T(\d+H)?(\d+M)?(\d+S)?)?|(\d+W))$",
                )
        }
        "ipv4" => re_match(
            s,
            &RE_IPV4,
            r"^(?:(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\.){3}(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)$",
        ),
        "ipv6" => re_match(
            s,
            &RE_IPV6,
            r"(?i)^((([0-9a-f]{1,4}:){7}([0-9a-f]{1,4}|:))|(([0-9a-f]{1,4}:){6}(:[0-9a-f]{1,4}|((25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3})|:))|(([0-9a-f]{1,4}:){5}(((:[0-9a-f]{1,4}){1,2})|:((25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3})|:))|(([0-9a-f]{1,4}:){4}(((:[0-9a-f]{1,4}){1,3})|((:[0-9a-f]{1,4})?:((25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}))|:))|(([0-9a-f]{1,4}:){3}(((:[0-9a-f]{1,4}){1,4})|((:[0-9a-f]{1,4}){0,2}:((25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}))|:))|(([0-9a-f]{1,4}:){2}(((:[0-9a-f]{1,4}){1,5})|((:[0-9a-f]{1,4}){0,3}:((25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}))|:))|(([0-9a-f]{1,4}:){1}(((:[0-9a-f]{1,4}){1,6})|((:[0-9a-f]{1,4}){0,4}:((25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}))|:))|(:(((:[0-9a-f]{1,4}){1,7})|((:[0-9a-f]{1,4}){0,5}:((25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}))|:)))$",
        ),
        "hostname" => {
            let t = s.strip_suffix('.').unwrap_or(s);
            !t.is_empty()
                && t.len() <= 253
                && re_match(
                    s,
                    &RE_HOST,
                    r"(?i)^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[-0-9a-z]{0,61}[0-9a-z])?)*\.?$",
                )
        }
        "idn-hostname" => {
            !s.is_empty()
                && s.len() <= 253
                && s.split('.').all(|l| {
                    !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-')
                })
        }
        "json-pointer" => re_match(s, &RE_JP, r"^(?:/(?:[^~/]|~0|~1)*)*$"),
        "relative-json-pointer" => re_match(
            s,
            &RE_RJP,
            r"^(?:0|[1-9][0-9]*)(?:#|(?:/(?:[^~/]|~0|~1)*)*)$",
        ),
        "byte" => re_match(
            s,
            &RE_B64,
            r"^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$",
        ),
        "regex" => Regex::new(s).is_ok(),
        _ => true,
    }
}

fn re_match(s: &str, lock: &OnceLock<Regex>, pattern: &str) -> bool {
    lock.get_or_init(|| Regex::new(pattern).unwrap())
        .is_match(s)
}

static RE_EMAIL: OnceLock<Regex> = OnceLock::new();
static RE_IDN_EMAIL: OnceLock<Regex> = OnceLock::new();
static RE_URI: OnceLock<Regex> = OnceLock::new();
static RE_URL: OnceLock<Regex> = OnceLock::new();
static RE_URI_TPL: OnceLock<Regex> = OnceLock::new();
static RE_IRI: OnceLock<Regex> = OnceLock::new();
static RE_UUID: OnceLock<Regex> = OnceLock::new();
static RE_DT: OnceLock<Regex> = OnceLock::new();
static RE_DATE: OnceLock<Regex> = OnceLock::new();
static RE_TIME: OnceLock<Regex> = OnceLock::new();
static RE_DUR: OnceLock<Regex> = OnceLock::new();
static RE_IPV4: OnceLock<Regex> = OnceLock::new();
static RE_IPV6: OnceLock<Regex> = OnceLock::new();
static RE_HOST: OnceLock<Regex> = OnceLock::new();
static RE_JP: OnceLock<Regex> = OnceLock::new();
static RE_RJP: OnceLock<Regex> = OnceLock::new();
static RE_B64: OnceLock<Regex> = OnceLock::new();

pub fn supported_formats() -> &'static [&'static str] {
    &[
        "email",
        "idn-email",
        "uri",
        "url",
        "uri-reference",
        "uri-template",
        "iri",
        "iri-reference",
        "uuid",
        "date-time",
        "date",
        "time",
        "duration",
        "ipv4",
        "ipv6",
        "hostname",
        "idn-hostname",
        "json-pointer",
        "relative-json-pointer",
        "byte",
        "regex",
        "password",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_formats() {
        assert!(validate("test@example.com", "email"));
        assert!(!validate("bad", "email"));
        assert!(validate("550e8400-e29b-41d4-a716-446655440000", "uuid"));
        assert!(validate("2024-01-15T10:30:00Z", "date-time"));
        assert!(validate("2024-01-15", "date"));
        assert!(validate("10:30:00Z", "time"));
        assert!(validate("P1Y2M3D", "duration"));
        assert!(validate("192.168.1.1", "ipv4"));
        assert!(!validate("256.1.1.1", "ipv4"));
        assert!(validate("::1", "ipv6"));
        assert!(validate("example.com", "hostname"));
        assert!(validate("https://example.com", "uri"));
        assert!(validate("https://example.com", "url"));
        assert!(validate("", "json-pointer"));
        assert!(validate("/foo/bar", "json-pointer"));
        assert!(validate("SGVsbG8=", "byte"));
        assert!(validate("^[a-z]+$", "regex"));
        assert!(!validate("[invalid", "regex"));
        assert!(validate("anything", "password"));
        assert!(validate("example.com", "idn-hostname"));
        assert!(validate("0/foo", "relative-json-pointer"));
        assert!(validate("https://example.com", "iri"));
        assert_eq!(supported_formats().len(), 22);
    }
}
