//! Deterministic detectors. Each one finds spans of private information in a
//! piece of text. They run before any optional agent review.
//!
//! Test strings that look like real credentials are assembled at runtime so the
//! source never contains a literal token that a secret scanner would flag.

use std::net::Ipv6Addr;
use std::sync::OnceLock;

use regex::{Regex, RegexBuilder};

use super::troveignore::TroveIgnore;
use super::{Category, Confidence};

/// One detected span.
#[derive(Debug, Clone)]
pub struct Detection {
    pub start: usize,
    pub end: usize,
    pub category: Category,
    pub confidence: Confidence,
    pub detector: &'static str,
    pub reason: &'static str,
    /// Fixed replacement instead of a numbered placeholder (home paths become `~`).
    pub replacement: Option<String>,
}

type RunFn = Box<dyn Fn(&str, &mut Vec<Detection>) + Send + Sync>;

pub struct Detector {
    pub name: &'static str,
    run: RunFn,
}

impl Detector {
    pub fn run(&self, text: &str, out: &mut Vec<Detection>) {
        (self.run)(text, out)
    }
}

/// The local machine identity whose name and home directory must not leak.
#[derive(Debug, Clone)]
pub struct LocalIdentity {
    pub username: Option<String>,
    pub home: Option<String>,
}

impl LocalIdentity {
    pub fn from_env() -> Self {
        let username = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .or_else(|_| std::env::var("LOGNAME"))
            .ok()
            .filter(|s| !s.is_empty());
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()
            .filter(|s| !s.is_empty());
        Self { username, home }
    }
}

fn rx(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid detector regex")
}

fn det(
    start: usize,
    end: usize,
    category: Category,
    confidence: Confidence,
    detector: &'static str,
    reason: &'static str,
) -> Detection {
    Detection {
        start,
        end,
        category,
        confidence,
        detector,
        reason,
        replacement: None,
    }
}

/// A detector over one regex. `group` selects the capture that is redacted and
/// `check` can reject a candidate or adjust its confidence.
fn regex_detector<F>(
    name: &'static str,
    pattern: &str,
    group: usize,
    category: Category,
    reason: &'static str,
    check: F,
) -> Detector
where
    F: Fn(&str, usize, usize) -> Option<Confidence> + Send + Sync + 'static,
{
    let re = rx(pattern);
    Detector {
        name,
        run: Box::new(move |text, out| {
            for caps in re.captures_iter(text) {
                let Some(m) = caps.get(group) else { continue };
                if let Some(conf) = check(text, m.start(), m.end()) {
                    out.push(det(m.start(), m.end(), category, conf, name, reason));
                }
            }
        }),
    }
}

fn high(_: &str, _: usize, _: usize) -> Option<Confidence> {
    Some(Confidence::High)
}

/// Every built-in detector, compiled once.
pub fn builtin() -> &'static [Detector] {
    static BUILTIN: OnceLock<Vec<Detector>> = OnceLock::new();
    BUILTIN.get_or_init(|| {
        vec![
            private_key(),
            regex_detector(
                "aws-access-key-id",
                r"\b(?:AKIA|ASIA|AGPA|AIDA|AROA|ANPA|ANVA|AIPA)[0-9A-Z]{16}\b",
                0,
                Category::Secret,
                "AWS access key ID",
                high,
            ),
            regex_detector(
                "aws-secret-access-key",
                r#"(?i)aws_?secret_?access_?key["']?\s*(?::|=|=>)\s*["']?([A-Za-z0-9/+=]{40})\b"#,
                1,
                Category::Secret,
                "AWS secret access key",
                high,
            ),
            regex_detector(
                "github-token",
                r"\b(?:(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{36,255}|github_pat_[A-Za-z0-9_]{40,255})\b",
                0,
                Category::Secret,
                "GitHub access token",
                high,
            ),
            regex_detector(
                "gitlab-token",
                r"\bglpat-[A-Za-z0-9_-]{20,}",
                0,
                Category::Secret,
                "GitLab personal access token",
                high,
            ),
            regex_detector(
                "slack-token",
                r"\bxox[abposr]-[A-Za-z0-9-]{10,}",
                0,
                Category::Secret,
                "Slack token",
                high,
            ),
            regex_detector(
                "slack-webhook",
                r"https://hooks\.slack\.com/services/[A-Za-z0-9_/]+",
                0,
                Category::Secret,
                "Slack incoming webhook URL",
                high,
            ),
            regex_detector(
                "stripe-key",
                r"\b(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{16,}\b",
                0,
                Category::Secret,
                "Stripe secret or restricted key",
                high,
            ),
            regex_detector(
                "anthropic-api-key",
                r"\bsk-ant-[A-Za-z0-9_-]{20,}",
                0,
                Category::Secret,
                "Anthropic API key",
                high,
            ),
            regex_detector(
                "openai-api-key",
                r"\bsk-(?:proj-|svcacct-|admin-)?[A-Za-z0-9_-]{20,}",
                0,
                Category::Secret,
                "OpenAI style API key",
                |text, s, e| {
                    let v = &text[s..e];
                    (!v.starts_with("sk-ant-") && token_like(v)).then_some(Confidence::High)
                },
            ),
            regex_detector(
                "google-api-key",
                r"\bAIza[0-9A-Za-z_-]{35}",
                0,
                Category::Secret,
                "Google API key",
                high,
            ),
            regex_detector(
                "npm-token",
                r"\bnpm_[A-Za-z0-9]{36}\b",
                0,
                Category::Secret,
                "npm access token",
                high,
            ),
            regex_detector(
                "pypi-token",
                r"\bpypi-AgEIcHlwaS5vcmc[A-Za-z0-9_-]{50,}",
                0,
                Category::Secret,
                "PyPI upload token",
                high,
            ),
            regex_detector(
                "sendgrid-api-key",
                r"\bSG\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9_-]{43}\b",
                0,
                Category::Secret,
                "SendGrid API key",
                high,
            ),
            regex_detector(
                "jwt",
                r"\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
                0,
                Category::Secret,
                "JSON Web Token",
                high,
            ),
            regex_detector(
                "url-credentials",
                r#"\b[A-Za-z][A-Za-z0-9+.-]*://[^\s:/@'"]+:([^\s@/'"]+)@"#,
                1,
                Category::Secret,
                "password embedded in a URL",
                |text, s, e| (!is_placeholder(&text[s..e])).then_some(Confidence::High),
            ),
            regex_detector(
                "bearer-token",
                r"(?i)\bbearer\s+([A-Za-z0-9\-._~+/]{20,}=*)",
                1,
                Category::Secret,
                "bearer token in an Authorization header",
                |text, s, e| {
                    let v = &text[s..e];
                    if is_placeholder(v) {
                        None
                    } else if token_like(v) {
                        Some(Confidence::High)
                    } else {
                        Some(Confidence::Medium)
                    }
                },
            ),
            secret_assignment(),
            high_entropy_string(),
            email(),
            phone(),
            internal_hostname(),
            ipv4(),
            ipv6(),
            home_path(),
            windows_home_path(),
            company_suffix(),
            customer_keyword(),
        ]
    })
}

/// Names of every built-in detector, for documentation and tests.
pub fn builtin_names() -> Vec<&'static str> {
    builtin().iter().map(|d| d.name).collect()
}

fn private_key() -> Detector {
    regex_detector(
        "private-key",
        r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----(?:.*?-----END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----|[A-Za-z0-9+/=\r\n]*)",
        0,
        Category::Secret,
        "private key material",
        high,
    )
}

fn secret_assignment() -> Detector {
    let re = rx(
        r#"(?i)\b([a-z0-9_.-]*?(?:password|passwd|pwd|secret|token|api[_-]?key|apikey|access[_-]?key|private[_-]?key|client[_-]?secret|auth[_-]?key|credential)s?[a-z0-9_.-]*)["']?[ \t]*(?::=|=>|:|=)[ \t]*["'`]?([^\s"'`,;)\]}]{8,})"#,
    );
    let name = "secret-assignment";
    Detector {
        name,
        run: Box::new(move |text, out| {
            for caps in re.captures_iter(text) {
                let (Some(key), Some(val)) = (caps.get(1), caps.get(2)) else {
                    continue;
                };
                let key = key.as_str().to_ascii_lowercase();
                const NOT_SECRET_KEYS: &[&str] = &[
                    "url", "uri", "path", "file", "hash", "length", "len", "min", "max", "policy",
                    "reset", "field", "label", "header", "type", "name", "count", "expir", "ttl",
                    "endpoint", "env", "prompt", "_id", "format", "limit", "size",
                ];
                if NOT_SECRET_KEYS.iter().any(|k| key.contains(k)) {
                    continue;
                }
                let v = val.as_str();
                if is_placeholder(v) || looks_like_code(v) {
                    continue;
                }
                let h = entropy(v);
                if h < 2.5 {
                    continue;
                }
                let conf = if (h >= 3.5 && v.len() >= 16) || (token_like(v) && v.len() >= 12) {
                    Confidence::High
                } else {
                    Confidence::Medium
                };
                out.push(det(
                    val.start(),
                    val.end(),
                    Category::Secret,
                    conf,
                    name,
                    "value assigned to a secret-looking key",
                ));
            }
        }),
    }
}

fn high_entropy_string() -> Detector {
    let re = rx(r"[A-Za-z0-9+/_-]{32,}={0,2}");
    let name = "high-entropy-string";
    Detector {
        name,
        run: Box::new(move |text, out| {
            for m in re.find_iter(text) {
                let prev = text[..m.start()].chars().next_back();
                if matches!(prev, Some(c) if c.is_alphanumeric() || "+/_-.\\".contains(c)) {
                    continue;
                }
                let v = m.as_str();
                if v.matches('/').count() > 2 || v.chars().all(|c| c.is_ascii_hexdigit()) {
                    continue;
                }
                if token_like(v) && entropy(v) >= 4.5 {
                    out.push(det(
                        m.start(),
                        m.end(),
                        Category::Secret,
                        Confidence::Medium,
                        name,
                        "long high-entropy string that looks like a key",
                    ));
                }
            }
        }),
    }
}

fn email() -> Detector {
    let re = rx(
        r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?)*\.[A-Za-z]{2,24}\b",
    );
    let name = "email";
    Detector {
        name,
        run: Box::new(move |text, out| {
            for m in re.find_iter(text) {
                let v = m.as_str();
                let Some((local, domain)) = v.rsplit_once('@') else {
                    continue;
                };
                // `scheme://user:pass@host` is URL userinfo, not an address.
                let token_start = text[..m.start()]
                    .rfind(|c: char| c.is_whitespace() || "\"'`(<".contains(c))
                    .map(|i| i + 1)
                    .unwrap_or(0);
                if text[token_start..m.start()].contains("://") {
                    continue;
                }
                let domain = domain.to_ascii_lowercase();
                if is_reserved_domain(&domain) {
                    continue;
                }
                if local == "git"
                    && matches!(
                        domain.as_str(),
                        "github.com" | "gitlab.com" | "bitbucket.org"
                    )
                {
                    continue;
                }
                let tld = domain.rsplit('.').next().unwrap_or("");
                const FILE_EXTS: &[&str] = &[
                    "png", "jpg", "jpeg", "gif", "svg", "webp", "ico", "js", "ts", "css", "rs",
                    "py", "json", "html", "lock", "toml", "yaml", "yml",
                ];
                if FILE_EXTS.contains(&tld) {
                    continue;
                }
                out.push(det(
                    m.start(),
                    m.end(),
                    Category::Email,
                    Confidence::Medium,
                    name,
                    "email address",
                ));
            }
        }),
    }
}

fn phone() -> Detector {
    let nanp = rx(r"(?:\+?1[ .-]?)?(?:\(\d{3}\)[ ]?|\d{3}[ .-])\d{3}[ .-]\d{4}");
    let intl = rx(r"\+\d{1,3}(?:[ .-]?\(?\d{1,5}\)?){1,5}");
    let name = "phone";
    Detector {
        name,
        run: Box::new(move |text, out| {
            let mut found: Vec<(usize, usize)> = Vec::new();
            for re in [&nanp, &intl] {
                for m in re.find_iter(text) {
                    let digits = m.as_str().chars().filter(|c| c.is_ascii_digit()).count();
                    if !(10..=15).contains(&digits) {
                        continue;
                    }
                    let prev = text[..m.start()].chars().next_back();
                    if matches!(prev, Some(c) if c.is_alphanumeric() || "._-/#".contains(c)) {
                        continue;
                    }
                    let rest = &text[m.end()..];
                    let mut next = rest.chars();
                    match next.next() {
                        Some(c) if c.is_alphanumeric() || c == '-' || c == '_' => continue,
                        Some('.') if next.next().is_some_and(|c| c.is_ascii_digit()) => continue,
                        _ => {}
                    }
                    if found.iter().any(|&(s, e)| m.start() < e && m.end() > s) {
                        continue;
                    }
                    found.push((m.start(), m.end()));
                    out.push(det(
                        m.start(),
                        m.end(),
                        Category::Phone,
                        Confidence::Medium,
                        name,
                        "phone number",
                    ));
                }
            }
        }),
    }
}

fn internal_hostname() -> Detector {
    const LABEL: &str = r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?";
    let tld = rx(&format!(
        r"(?i)\b(?:{LABEL}\.)+(?:internal|intranet|corp|lan|local|localdomain|private|home\.arpa)\b"
    ));
    let sub = rx(&format!(
        r"(?i)\b(?:{LABEL}\.)+(?:internal|intranet|corp|int|priv|private|lan)\.(?:{LABEL}\.)*[a-z]{{2,24}}\b"
    ));
    let name = "internal-hostname";
    Detector {
        name,
        run: Box::new(move |text, out| {
            for (re, is_tld) in [(&sub, false), (&tld, true)] {
                for m in re.find_iter(text) {
                    let prev = text[..m.start()].chars().next_back();
                    if matches!(prev, Some('.' | '_')) {
                        continue;
                    }
                    let v = m.as_str().to_ascii_lowercase();
                    if is_tld {
                        let mut rest = text[m.end()..].chars();
                        if rest.next() == Some('.')
                            && rest.next().is_some_and(|c| c.is_alphanumeric())
                        {
                            continue;
                        }
                        let first = v.split('.').next().unwrap_or("");
                        const CONFIG_WORDS: &[&str] = &[
                            "env", "settings", "config", "docker", "compose", "values", "app",
                            "vite", "next", "webpack",
                        ];
                        if v.ends_with(".local") && CONFIG_WORDS.contains(&first) {
                            continue;
                        }
                    }
                    out.push(det(
                        m.start(),
                        m.end(),
                        Category::InternalHost,
                        Confidence::Medium,
                        name,
                        "internal hostname",
                    ));
                }
            }
        }),
    }
}

fn ipv4() -> Detector {
    let re = rx(r"\b(?:\d{1,3}\.){3}\d{1,3}\b");
    let name = "ip-address";
    Detector {
        name,
        run: Box::new(move |text, out| {
            for m in re.find_iter(text) {
                let prev = text[..m.start()].chars().next_back();
                if matches!(prev, Some('.')) {
                    continue;
                }
                let mut rest = text[m.end()..].chars();
                if rest.next() == Some('.') && rest.next().is_some_and(|c| c.is_ascii_digit()) {
                    continue;
                }
                let octets: Vec<u32> = m
                    .as_str()
                    .split('.')
                    .filter_map(|o| o.parse().ok())
                    .collect();
                if octets.len() != 4 || octets.iter().any(|&o| o > 255) {
                    continue;
                }
                let (conf, reason) = match (octets[0], octets[1], octets[2]) {
                    (127, _, _) | (0, _, _) | (255, 255, _) => continue,
                    (192, 0, 2) | (198, 51, 100) | (203, 0, 113) => continue,
                    (10, _, _) | (192, 168, _) | (169, 254, _) => {
                        (Confidence::Medium, "private network address")
                    }
                    (172, b, _) if (16..=31).contains(&b) => {
                        (Confidence::Medium, "private network address")
                    }
                    (100, b, _) if (64..=127).contains(&b) => {
                        (Confidence::Medium, "carrier-grade NAT address")
                    }
                    _ => (Confidence::Low, "public IP address"),
                };
                out.push(det(
                    m.start(),
                    m.end(),
                    Category::IpAddress,
                    conf,
                    name,
                    reason,
                ));
            }
        }),
    }
}

fn ipv6() -> Detector {
    let re = rx(r"(?i)[0-9a-f:]{2,39}");
    let name = "ipv6-address";
    Detector {
        name,
        run: Box::new(move |text, out| {
            for m in re.find_iter(text) {
                let v = m.as_str();
                let prev = text[..m.start()].chars().next_back();
                let next = text[m.end()..].chars().next();
                if matches!(prev, Some(c) if c.is_alphanumeric() || c == '_')
                    || matches!(next, Some(c) if c.is_alphanumeric() || c == '_')
                {
                    continue;
                }
                let groups = v.split(':').filter(|g| !g.is_empty()).count();
                if !(v.contains("::") || v.matches(':').count() >= 7)
                    || groups < 2
                    || !v.chars().any(|c| c.is_ascii_digit())
                {
                    continue;
                }
                let Ok(addr) = v.parse::<Ipv6Addr>() else {
                    continue;
                };
                let seg = addr.segments();
                if addr.is_loopback()
                    || addr.is_unspecified()
                    || (seg[0] == 0x2001 && seg[1] == 0x0db8)
                {
                    continue;
                }
                let (conf, reason) = if (seg[0] & 0xfe00) == 0xfc00 || (seg[0] & 0xffc0) == 0xfe80 {
                    (Confidence::Medium, "private or link-local IPv6 address")
                } else {
                    (Confidence::Low, "public IPv6 address")
                };
                out.push(det(
                    m.start(),
                    m.end(),
                    Category::IpAddress,
                    conf,
                    name,
                    reason,
                ));
            }
        }),
    }
}

fn home_path() -> Detector {
    let re = rx(r"(?:/Users|/home)/[A-Za-z0-9._-]+");
    let name = "home-path";
    Detector {
        name,
        run: Box::new(move |text, out| {
            for m in re.find_iter(text) {
                let prev = text[..m.start()].chars().next_back();
                if matches!(prev, Some(c) if c.is_alphanumeric() || "._-".contains(c)) {
                    continue;
                }
                let mut d = det(
                    m.start(),
                    m.end(),
                    Category::LocalPath,
                    Confidence::Medium,
                    name,
                    "home directory path reveals a local username",
                );
                d.replacement = Some("~".into());
                out.push(d);
            }
        }),
    }
}

fn windows_home_path() -> Detector {
    let re = rx(
        r#"(?i)\b[A-Z]:(?:\\{1,2}|/)(?:Users|Documents and Settings)(?:\\{1,2}|/)[^\\/\s"'<>|:*?]+"#,
    );
    let name = "windows-home-path";
    Detector {
        name,
        run: Box::new(move |text, out| {
            for m in re.find_iter(text) {
                let mut d = det(
                    m.start(),
                    m.end(),
                    Category::LocalPath,
                    Confidence::Medium,
                    name,
                    "Windows profile path reveals a local username",
                );
                d.replacement = Some("~".into());
                out.push(d);
            }
        }),
    }
}

const NAME_STOPWORDS: &[&str] = &[
    "The",
    "A",
    "An",
    "At",
    "For",
    "From",
    "With",
    "By",
    "To",
    "Of",
    "Contact",
    "Call",
    "Email",
    "Ask",
    "See",
    "Our",
    "Their",
    "Your",
    "Customer",
    "Client",
    "And",
    "Or",
    "In",
    "On",
    "Per",
    "Via",
    "Copyright",
    "Built",
    "Made",
    "Thanks",
];

fn company_suffix() -> Detector {
    let re = rx(
        r"\b((?:[A-Z][A-Za-z0-9&'-]*[ \t]+){0,3}[A-Z][A-Za-z0-9&'-]*),?[ \t]+(?:(?:Inc|LLC|Ltd|Limited|GmbH|Corp|Corporation|PLC|Pty|AG|SAS|SARL|BV)\b\.?|S\.A\.|B\.V\.|Co\.)",
    );
    let name = "company-suffix";
    Detector {
        name,
        run: Box::new(move |text, out| {
            for caps in re.captures_iter(text) {
                let (Some(all), Some(words)) = (caps.get(0), caps.get(1)) else {
                    continue;
                };
                // Drop leading sentence words such as "Contact" or "The".
                let mut start = words.start();
                for w in words.as_str().split_whitespace() {
                    if NAME_STOPWORDS.contains(&w) {
                        let idx = text[start..]
                            .find(w)
                            .map(|i| start + i + w.len())
                            .unwrap_or(start);
                        start = idx + text[idx..].len() - text[idx..].trim_start().len();
                    } else {
                        break;
                    }
                }
                if start >= words.end() {
                    continue;
                }
                out.push(det(
                    start,
                    all.end(),
                    Category::CustomerName,
                    Confidence::Medium,
                    name,
                    "organization name with a legal suffix",
                ));
            }
        }),
    }
}

fn customer_keyword() -> Detector {
    let labeled = rx(
        r#"(?i:\b(?:customer|client|account)(?:[ _-]?name)?)[ \t]*(?:[:=]|\bis\b)[ \t]*["']?([A-Z][A-Za-z0-9&'-]*(?:[ \t][A-Z][A-Za-z0-9&'-]*){0,3})"#,
    );
    let inline = rx(
        r"(?i:\bfor (?:the )?(?:customer|client)) ([A-Z][A-Za-z0-9&'-]*(?:[ \t][A-Z][A-Za-z0-9&'-]*){0,2})",
    );
    let name = "customer-keyword";
    Detector {
        name,
        run: Box::new(move |text, out| {
            for re in [&labeled, &inline] {
                for caps in re.captures_iter(text) {
                    let Some(m) = caps.get(1) else { continue };
                    const NOT_NAMES: &[&str] = &[
                        "Client", "Customer", "None", "Null", "String", "True", "False", "Name",
                        "Id", "ID", "TBD", "Unknown", "Example", "Secret", "Portal", "Side",
                    ];
                    if NOT_NAMES.contains(&m.as_str()) {
                        continue;
                    }
                    out.push(det(
                        m.start(),
                        m.end(),
                        Category::CustomerName,
                        Confidence::Medium,
                        name,
                        "named as a customer or client",
                    ));
                }
            }
        }),
    }
}

/// Detectors built from `.troveignore` deny, customer and internal-domain rules.
pub fn from_ignore(ignore: &TroveIgnore) -> Vec<Detector> {
    let mut out = Vec::new();
    for term in &ignore.customers {
        if let Some(re) = literal_regex(term) {
            out.push(literal_detector(
                "troveignore-customer",
                re,
                Category::CustomerName,
                Confidence::High,
                "customer listed in .troveignore",
                None,
            ));
        }
    }
    for term in &ignore.deny {
        if let Some(re) = literal_regex(term) {
            out.push(literal_detector(
                "troveignore-deny",
                re,
                Category::Denylisted,
                Confidence::High,
                "term denied in .troveignore",
                None,
            ));
        }
    }
    for re in &ignore.deny_regex {
        out.push(literal_detector(
            "troveignore-deny-regex",
            re.clone(),
            Category::Denylisted,
            Confidence::Medium,
            "pattern denied in .troveignore",
            None,
        ));
    }
    for domain in &ignore.internal_domains {
        let pattern = format!(r"(?i)\b(?:[a-z0-9-]+\.)*{}\b", regex::escape(domain));
        if let Ok(re) = Regex::new(&pattern) {
            out.push(literal_detector(
                "troveignore-internal-domain",
                re,
                Category::InternalHost,
                Confidence::High,
                "domain marked internal in .troveignore",
                None,
            ));
        }
    }
    out
}

/// Detectors for the current user's name and home directory.
pub fn from_identity(identity: &LocalIdentity) -> Vec<Detector> {
    let mut out = Vec::new();
    if let Some(home) = identity
        .home
        .as_deref()
        .filter(|h| h.len() > 1 && *h != "/")
    {
        out.push(literal_detector(
            "local-home",
            path_regex(home),
            Category::LocalPath,
            Confidence::Medium,
            "current user's home directory",
            Some("~".into()),
        ));
    }
    if let Some(user) = identity.username.as_deref() {
        const GENERIC: &[&str] = &[
            "root", "user", "admin", "runner", "ubuntu", "ec2-user", "vagrant", "dev", "test",
            "builder", "node", "app", "build", "ci", "docker", "default",
        ];
        if user.len() >= 3 && !GENERIC.contains(&user.to_ascii_lowercase().as_str()) {
            if let Some(re) = literal_regex(user) {
                out.push(literal_detector(
                    "local-username",
                    re,
                    Category::Username,
                    Confidence::Medium,
                    "current user's login name",
                    None,
                ));
            }
        }
    }
    out
}

/// Detectors that replace fixed absolute paths (such as the repository root)
/// with a stable label.
pub fn from_path_labels(labels: &[(String, String)]) -> Vec<Detector> {
    labels
        .iter()
        .filter(|(path, _)| path.len() > 1)
        .map(|(path, label)| {
            literal_detector(
                "path-label",
                path_regex(path),
                Category::LocalPath,
                Confidence::High,
                "absolute path on the build machine",
                Some(label.clone()),
            )
        })
        .collect()
}

/// An absolute path that must not run on into a longer name: `/root` matches
/// `/root/x` but not `/rootfs`.
fn path_regex(path: &str) -> Regex {
    let path = path.trim_end_matches(['/', '\\']);
    let trail = if path.ends_with(|c: char| c.is_alphanumeric() || c == '_') {
        r"\b"
    } else {
        ""
    };
    Regex::new(&format!("{}{trail}", regex::escape(path))).expect("escaped literal")
}

fn literal_regex(term: &str) -> Option<Regex> {
    let term = term.trim();
    let first = term.chars().next()?;
    let last = term.chars().last()?;
    let lead = if first.is_alphanumeric() || first == '_' {
        r"\b"
    } else {
        ""
    };
    let trail = if last.is_alphanumeric() || last == '_' {
        r"\b"
    } else {
        ""
    };
    RegexBuilder::new(&format!("{lead}{}{trail}", regex::escape(term)))
        .case_insensitive(true)
        .build()
        .ok()
}

fn literal_detector(
    name: &'static str,
    re: Regex,
    category: Category,
    confidence: Confidence,
    reason: &'static str,
    replacement: Option<String>,
) -> Detector {
    Detector {
        name,
        run: Box::new(move |text, out| {
            for m in re.find_iter(text) {
                if m.start() == m.end() {
                    continue;
                }
                let mut d = det(m.start(), m.end(), category, confidence, name, reason);
                d.replacement = replacement.clone();
                out.push(d);
            }
        }),
    }
}

/// Shannon entropy in bits per character.
pub fn entropy(s: &str) -> f64 {
    let mut counts = std::collections::HashMap::new();
    let mut n = 0usize;
    for c in s.chars() {
        *counts.entry(c).or_insert(0usize) += 1;
        n += 1;
    }
    if n == 0 {
        return 0.0;
    }
    counts
        .values()
        .map(|&c| {
            let p = c as f64 / n as f64;
            -p * p.log2()
        })
        .sum()
}

/// Mixed upper case, lower case and digits, the shape of generated tokens.
fn token_like(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_uppercase())
        && s.chars().any(|c| c.is_ascii_lowercase())
        && s.chars().any(|c| c.is_ascii_digit())
}

fn is_placeholder(v: &str) -> bool {
    let l = v.to_ascii_lowercase();
    const WORDS: &[&str] = &[
        "changeme",
        "change_me",
        "example",
        "placeholder",
        "your_",
        "your-",
        "redacted",
        "xxxx",
        "****",
        "dummy",
        "sample",
        "<",
        "${",
        "{{",
        "password",
        "secret",
        "token",
        "todo",
    ];
    WORDS.iter().any(|w| l.contains(w)) || v.chars().all(|c| c == v.chars().next().unwrap_or(c))
}

fn looks_like_code(v: &str) -> bool {
    static DOTTED: OnceLock<Regex> = OnceLock::new();
    let dotted =
        DOTTED.get_or_init(|| rx(r"^[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)+$"));
    v.contains(['(', '[', '{', '<', '$', '%', '&', '|'])
        || v.starts_with("http://")
        || v.starts_with("https://")
        || dotted.is_match(v)
        || v.chars()
            .all(|c| c.is_ascii_lowercase() || c == '_' || c == '-')
        || v.chars().all(|c| c.is_ascii_uppercase() || c == '_')
        || matches!(
            v.to_ascii_lowercase().as_str(),
            "true" | "false" | "null" | "none" | "undefined"
        )
}

fn is_reserved_domain(domain: &str) -> bool {
    matches!(
        domain,
        "example.com" | "example.org" | "example.net" | "localhost"
    ) || [
        ".example",
        ".test",
        ".invalid",
        ".localhost",
        ".example.com",
        ".example.org",
        ".example.net",
    ]
    .iter()
    .any(|s| domain.ends_with(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(name: &str, text: &str) -> Vec<String> {
        let d = builtin()
            .iter()
            .find(|d| d.name == name)
            .expect("detector exists");
        let mut out = Vec::new();
        d.run(text, &mut out);
        out.iter()
            .map(|x| text[x.start..x.end].to_string())
            .collect()
    }

    fn run_conf(name: &str, text: &str) -> Vec<Confidence> {
        let d = builtin()
            .iter()
            .find(|d| d.name == name)
            .expect("detector exists");
        let mut out = Vec::new();
        d.run(text, &mut out);
        out.iter().map(|x| x.confidence).collect()
    }

    fn cat(parts: &[&str]) -> String {
        parts.concat()
    }

    #[test]
    fn private_key_block() {
        let key = cat(&[
            "-----BEGIN RSA ",
            "PRIVATE KEY-----\nMIIEow\nIBAAKC\n-----END RSA ",
            "PRIVATE KEY-----",
        ]);
        let text = format!("before\n{key}\nafter");
        assert_eq!(run("private-key", &text), vec![key]);
        let truncated = cat(&["-----BEGIN OPENSSH ", "PRIVATE KEY-----\nb3BlbnNzaC1rZXk\n"]);
        assert_eq!(run("private-key", &truncated).len(), 1);
        assert!(run("private-key", "-----BEGIN PUBLIC KEY-----").is_empty());
    }

    #[test]
    fn aws_keys() {
        let id = cat(&["AKIA", "Z7Q3XK2M9P4L8W1N"]);
        assert_eq!(run("aws-access-key-id", &format!("id={id}")), vec![id]);
        assert!(run("aws-access-key-id", "AKIA_not_a_key").is_empty());
        let secret = cat(&["wJalrXUtnFEMI/K7MDENG/", "bPxRfiCYzq8Wq2x9T3"]);
        assert_eq!(
            run(
                "aws-secret-access-key",
                &format!("aws_secret_access_key = \"{secret}\"")
            ),
            vec![secret]
        );
    }

    #[test]
    fn github_tokens() {
        let classic = cat(&["ghp_", &"aB3dE5".repeat(6)]);
        let fine = cat(&["github_pat_", &"11ABCDEFG0".repeat(5)]);
        assert_eq!(
            run("github-token", &format!("{classic} {fine}")),
            vec![classic, fine]
        );
        assert!(run("github-token", "ghp_short").is_empty());
    }

    #[test]
    fn gitlab_slack_stripe_tokens() {
        let gl = cat(&["glpat-", "Zx9_kLm2NpQr4StUv6Wy"]);
        assert_eq!(run("gitlab-token", &gl), vec![gl]);
        let slack = cat(&["xoxb-", "1234567890-abcdefABCDEF"]);
        assert_eq!(run("slack-token", &slack), vec![slack]);
        let hook = cat(&["https://hooks.slack.com/services/", "T000/B000/XXXXabcd"]);
        assert_eq!(run("slack-webhook", &hook), vec![hook]);
        let sk = cat(&["sk_live_", "4eC39HqLyjWDarjtT1zdp7dc"]);
        assert_eq!(run("stripe-key", &sk), vec![sk]);
        let pk = cat(&["pk_live_", "4eC39HqLyjWDarjtT1zdp7dc"]);
        assert!(run("stripe-key", &pk).is_empty());
    }

    #[test]
    fn ai_provider_keys() {
        let ant = cat(&["sk-ant-", "api03-Ab12Cd34Ef56Gh78Ij90"]);
        assert_eq!(run("anthropic-api-key", &ant), vec![ant.clone()]);
        assert!(run("openai-api-key", &ant).is_empty());
        let oai = cat(&["sk-proj-", "Ab12Cd34Ef56Gh78Ij90Kl"]);
        assert_eq!(run("openai-api-key", &oai), vec![oai]);
        assert!(run("openai-api-key", "sk-some-long-css-class-name-here").is_empty());
    }

    #[test]
    fn google_npm_pypi_sendgrid_keys() {
        let g = cat(&["AIza", "SyA1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q"]);
        assert_eq!(run("google-api-key", &g), vec![g]);
        let npm = cat(&["npm_", "aB3dE5aB3dE5aB3dE5aB3dE5aB3dE5aB3dE5"]);
        assert_eq!(run("npm-token", &npm), vec![npm]);
        let pypi = cat(&["pypi-", "AgEIcHlwaS5vcmc", &"Ab1_".repeat(14)]);
        assert_eq!(run("pypi-token", &pypi), vec![pypi]);
        let sg = cat(&[
            "SG.",
            "abcdefghijABCDEFGHIJ12",
            ".",
            "abcdefghijABCDEFGHIJ1234567890abcdefghijABC",
        ]);
        assert_eq!(run("sendgrid-api-key", &sg), vec![sg]);
    }

    #[test]
    fn jwt_token() {
        let jwt = cat(&[
            "eyJ",
            "hbGciOiJIUzI1NiJ9",
            ".eyJ",
            "zdWIiOiIxMjM0NTY3ODkwIn0",
            ".",
            "dozjgNryP4J3jVmNHl0w5N",
        ]);
        assert_eq!(run("jwt", &format!("auth: {jwt}")), vec![jwt]);
        assert!(run("jwt", "eyJ.eyJ.x").is_empty());
    }

    #[test]
    fn url_credentials() {
        let url = cat(&["postgres://app:", "Pr0dPassw0rd", "@db.acme.io:5432/app"]);
        assert_eq!(
            run("url-credentials", &url),
            vec!["Pr0dPassw0rd".to_string()]
        );
        assert!(run("url-credentials", "postgres://app:${DB_PASS}@db/app").is_empty());
        assert!(run("url-credentials", "https://example.com/path").is_empty());
    }

    #[test]
    fn bearer_token() {
        let tok = cat(&["Abc123", "Def456Ghi789Jkl012"]);
        assert_eq!(
            run("bearer-token", &format!("Authorization: Bearer {tok}")),
            vec![tok]
        );
        assert!(run("bearer-token", "Bearer <your-token-here-please-ok>").is_empty());
    }

    #[test]
    fn secret_assignment_values() {
        let v = cat(&["q8Rz", "Lk2Vx9Tn4Wm7"]);
        assert_eq!(
            run("secret-assignment", &format!("DB_PASSWORD={v}")),
            vec![v.clone()]
        );
        assert_eq!(
            run_conf("secret-assignment", &format!("api_key: \"{v}\"")),
            vec![Confidence::High]
        );
        for benign in [
            "password = get_password()",
            "token: process.env.TOKEN",
            "secret = ${SECRET}",
            "password: changeme123",
            "password_reset_url = https://acme.io/reset",
            "token_type = bearer_token",
            "api_key = os.environ[\"API_KEY\"]",
            "TOKEN=SOME_CONSTANT",
        ] {
            assert!(run("secret-assignment", benign).is_empty(), "{benign}");
        }
    }

    #[test]
    fn high_entropy_strings() {
        let v = cat(&["Zq3Lr8Xw", "1Kp9Vt2Mn7Bc4Hd6Jf0Ga5Ys"]);
        assert_eq!(
            run("high-entropy-string", &format!("value {v} end")),
            vec![v]
        );
        assert!(run("high-entropy-string", &"a1b2c3d4e5f6".repeat(4)).is_empty());
        assert!(run(
            "high-entropy-string",
            "crates/trove-ship/src/redact/detectors"
        )
        .is_empty());
        assert!(run(
            "high-entropy-string",
            "ThisIsAVeryLongCamelCaseIdentifierName"
        )
        .is_empty());
    }

    #[test]
    fn emails() {
        assert_eq!(
            run("email", "ping jane.doe+ops@acme-billing.com now"),
            vec!["jane.doe+ops@acme-billing.com".to_string()]
        );
        assert!(run("email", "user@example.com and a@b.test").is_empty());
        assert!(run("email", "git@github.com:org/repo.git").is_empty());
        assert!(run("email", "logo@2x.png react@18.3.1").is_empty());
        assert!(run("email", "postgres://app:hunter2@db.acme.io/app").is_empty());
        assert_eq!(
            run("email", "mailto:ops@acme.io"),
            vec!["ops@acme.io".to_string()]
        );
    }

    #[test]
    fn phones() {
        assert_eq!(
            run("phone", "call (415) 555-0132."),
            vec!["(415) 555-0132".to_string()]
        );
        assert_eq!(
            run("phone", "cell +44 20 7946 0958"),
            vec!["+44 20 7946 0958".to_string()]
        );
        assert_eq!(
            run("phone", "tel: +919876543210"),
            vec!["+919876543210".to_string()]
        );
        assert_eq!(
            run("phone", "415.555.0132"),
            vec!["415.555.0132".to_string()]
        );
        for benign in [
            "v1.2.3",
            "2026-10-16",
            "10.0.12.7",
            "id 1234567890123456789",
            "port 8080",
        ] {
            assert!(run("phone", benign).is_empty(), "{benign}");
        }
    }

    #[test]
    fn internal_hostnames() {
        assert_eq!(
            run(
                "internal-hostname",
                "see https://jenkins.corp.acme-billing.com/job"
            ),
            vec!["jenkins.corp.acme-billing.com".to_string()]
        );
        assert_eq!(
            run("internal-hostname", "db1.prod.internal:5432"),
            vec!["db1.prod.internal".to_string()]
        );
        assert_eq!(
            run("internal-hostname", "redis.svc.cluster.local"),
            vec!["redis.svc.cluster.local".to_string()]
        );
        for benign in [
            "copy .env.local",
            "config.local.json",
            "settings.local",
            "example.com",
        ] {
            assert!(run("internal-hostname", benign).is_empty(), "{benign}");
        }
    }

    #[test]
    fn ipv4_addresses() {
        assert_eq!(
            run("ip-address", "host 10.0.12.7 up"),
            vec!["10.0.12.7".to_string()]
        );
        assert_eq!(
            run_conf("ip-address", "172.20.1.4"),
            vec![Confidence::Medium]
        );
        assert_eq!(run_conf("ip-address", "8.8.4.4"), vec![Confidence::Low]);
        for benign in [
            "127.0.0.1",
            "0.0.0.0",
            "192.0.2.10",
            "999.1.1.1",
            "1.2.3.4.5",
            "v1.2.3.4",
        ] {
            assert!(run("ip-address", benign).is_empty(), "{benign}");
        }
    }

    #[test]
    fn ipv6_addresses() {
        assert_eq!(
            run("ipv6-address", "addr fd12:3456:789a::1 ok"),
            vec!["fd12:3456:789a::1".to_string()]
        );
        assert_eq!(
            run_conf("ipv6-address", "fe80::1ff:fe23:4567:890a"),
            vec![Confidence::Medium]
        );
        for benign in ["::1", "2001:db8::1", "std::fs::read", "a::b", "12:30:45"] {
            assert!(run("ipv6-address", benign).is_empty(), "{benign}");
        }
    }

    #[test]
    fn home_paths() {
        assert_eq!(
            run("home-path", "at /Users/jdoe/src/app"),
            vec!["/Users/jdoe".to_string()]
        );
        assert_eq!(
            run("home-path", "/home/alice/.config"),
            vec!["/home/alice".to_string()]
        );
        assert!(run("home-path", "/home/<you>/x").is_empty());
        let d = builtin().iter().find(|d| d.name == "home-path").unwrap();
        let mut out = Vec::new();
        d.run("/home/alice", &mut out);
        assert_eq!(out[0].replacement.as_deref(), Some("~"));
    }

    #[test]
    fn windows_home_paths() {
        assert_eq!(
            run("windows-home-path", r"C:\Users\JDoe\repo"),
            vec![r"C:\Users\JDoe".to_string()]
        );
        assert_eq!(run("windows-home-path", r"c:\\Users\\jdoe\\x").len(), 1);
    }

    #[test]
    fn company_names() {
        assert_eq!(
            run("company-suffix", "Contact Globex Corporation about it"),
            vec!["Globex Corporation".to_string()]
        );
        assert_eq!(
            run("company-suffix", "built for Initech, LLC."),
            vec!["Initech, LLC.".to_string()]
        );
        assert!(run("company-suffix", "the inc file").is_empty());
    }

    #[test]
    fn customer_keywords() {
        assert_eq!(
            run("customer-keyword", "customer: Umbrella Health"),
            vec!["Umbrella Health".to_string()]
        );
        assert_eq!(
            run("customer-keyword", "built for customer Hooli to"),
            vec!["Hooli".to_string()]
        );
        assert!(run("customer-keyword", "client = Client::new()").is_empty());
        assert!(run("customer-keyword", "client: none").is_empty());
    }

    #[test]
    fn troveignore_detectors() {
        let ignore = TroveIgnore::parse(
            "customer: Vandelay\ndeny: Project Nightjar\ndeny-regex: TICKET-\\d+\ninternal-domain: acme-billing.com\n",
        )
        .unwrap();
        let dets = from_ignore(&ignore);
        let text = "vandelay asked about project nightjar in TICKET-42 on api.acme-billing.com";
        let mut out = Vec::new();
        for d in &dets {
            d.run(text, &mut out);
        }
        let found: Vec<_> = out
            .iter()
            .map(|d| (d.detector, &text[d.start..d.end]))
            .collect();
        assert!(found.contains(&("troveignore-customer", "vandelay")));
        assert!(found.contains(&("troveignore-deny", "project nightjar")));
        assert!(found.contains(&("troveignore-deny-regex", "TICKET-42")));
        assert!(found.contains(&("troveignore-internal-domain", "api.acme-billing.com")));
    }

    #[test]
    fn identity_detectors() {
        let id = LocalIdentity {
            username: Some("jdoe".into()),
            home: Some("/srv/people/jdoe".into()),
        };
        let dets = from_identity(&id);
        let text = "built in /srv/people/jdoe/work by jdoe";
        let mut out = Vec::new();
        for d in &dets {
            d.run(text, &mut out);
        }
        assert!(out
            .iter()
            .any(|d| d.detector == "local-home" && d.replacement.as_deref() == Some("~")));
        assert!(out
            .iter()
            .any(|d| d.detector == "local-username" && &text[d.start..d.end] == "jdoe"));
        let mut out = Vec::new();
        dets[0].run("/srv/people/jdoe2 is someone else", &mut out);
        assert!(out.is_empty());
        let generic = LocalIdentity {
            username: Some("root".into()),
            home: Some("/".into()),
        };
        assert!(from_identity(&generic).is_empty());
    }

    #[test]
    fn path_label_detector() {
        let dets = from_path_labels(&[("/opt/build/acme".into(), "$REPO_ROOT".into())]);
        let mut out = Vec::new();
        dets[0].run("file /opt/build/acme/src/main.rs", &mut out);
        assert_eq!(out[0].replacement.as_deref(), Some("$REPO_ROOT"));
        assert_eq!(out[0].confidence, Confidence::High);
    }

    #[test]
    fn every_builtin_detector_is_covered() {
        // Keep this list in sync with the tests above: adding a detector without a
        // test fails here.
        let tested = [
            "private-key",
            "aws-access-key-id",
            "aws-secret-access-key",
            "github-token",
            "gitlab-token",
            "slack-token",
            "slack-webhook",
            "stripe-key",
            "anthropic-api-key",
            "openai-api-key",
            "google-api-key",
            "npm-token",
            "pypi-token",
            "sendgrid-api-key",
            "jwt",
            "url-credentials",
            "bearer-token",
            "secret-assignment",
            "high-entropy-string",
            "email",
            "phone",
            "internal-hostname",
            "ip-address",
            "ipv6-address",
            "home-path",
            "windows-home-path",
            "company-suffix",
            "customer-keyword",
        ];
        let mut names = builtin_names();
        names.sort();
        let mut tested: Vec<_> = tested.to_vec();
        tested.sort();
        assert_eq!(names, tested);
    }

    #[test]
    fn entropy_is_sane() {
        assert_eq!(entropy(""), 0.0);
        assert_eq!(entropy("aaaa"), 0.0);
        assert!((entropy("ab") - 1.0).abs() < 1e-9);
    }
}
