use std::collections::HashMap;

pub(crate) const MAX_DISPLAY_CHARS: usize = 1_800;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CleanedWhois {
    pub text: String,
    pub truncated: bool,
}

enum Line {
    Raw(String),
    KeyValue { key: String, values: Vec<String> },
}

/// Applies JVM-compatible WHOIS cleanup rules and a final, character-based display limit.
pub(crate) fn clean_whois(raw: &str) -> CleanedWhois {
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = Vec::new();
    let mut key_index: HashMap<String, usize> = HashMap::new();

    for line in normalized.lines() {
        if is_boilerplate(line) {
            break;
        }

        let line = line.trim_end();
        if line.trim().is_empty() {
            lines.push(Line::Raw(String::new()));
            continue;
        }

        let Some((key, value)) = line.split_once(':') else {
            lines.push(Line::Raw(filter_controls(line)));
            continue;
        };
        let key = filter_controls(key.trim());
        let value = filter_controls(value.trim());
        if key.is_empty() || value.is_empty() {
            if value.is_empty() {
                continue;
            }
            lines.push(Line::Raw(filter_controls(line)));
            continue;
        }

        if let Some(index) = key_index.get(&key) {
            if let Some(Line::KeyValue { values, .. }) = lines.get_mut(*index) {
                values.push(value);
            }
        } else {
            key_index.insert(key.clone(), lines.len());
            lines.push(Line::KeyValue {
                key,
                values: vec![value],
            });
        }
    }

    let mut output = Vec::new();
    let mut blank_run = 0;
    for line in lines {
        match line {
            Line::Raw(line) if line.is_empty() => {
                blank_run += 1;
                if blank_run <= 2 {
                    output.push(String::new());
                }
            }
            Line::Raw(line) => {
                blank_run = 0;
                output.push(filter_controls(&line));
            }
            Line::KeyValue { key, values } => {
                blank_run = 0;
                output.push(format!("{key}: {}", values.join(", ")));
            }
        }
    }

    let text = output.join("\n").trim().to_owned();
    let chars: Vec<char> = text.chars().collect();
    if chars.len() > MAX_DISPLAY_CHARS {
        CleanedWhois {
            text: chars[..MAX_DISPLAY_CHARS].iter().collect(),
            truncated: true,
        }
    } else {
        CleanedWhois {
            text,
            truncated: false,
        }
    }
}

/// Recognizes the common "record does not exist" markers returned by registry servers.
pub(crate) fn is_not_found(text: &str) -> bool {
    let Some(first_line) = text.lines().find(|line| !line.trim().is_empty()) else {
        return true;
    };
    let first_line = first_line.trim().to_ascii_lowercase();
    first_line.starts_with("no match for")
        || first_line.starts_with("not found")
        || first_line.starts_with("domain not found")
        || first_line.starts_with("no entries found")
}

fn is_boilerplate(line: &str) -> bool {
    let line = line.trim_start();
    let lower = line.to_ascii_lowercase();
    lower.starts_with("for more information on whois status codes")
        || lower.starts_with("notice:")
        || lower.starts_with("terms of use:")
        || lower.starts_with("whois terms of use")
        || lower.starts_with(">>> last update of whois database:")
}

fn filter_controls(line: &str) -> String {
    line.chars()
        .filter(|character| !character.is_control() || *character == '\t')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_boilerplate_duplicates_and_blank_runs() {
        let raw = "Domain: EXAMPLE.COM\r\n\
                   Registrar: One\r\n\
                   Empty:\r\n\
                   Registrar: Two\r\n\
                   \r\n\r\n\r\n\
                   NOTICE: stop here\r\n\
                   After: ignored\r\n";
        let cleaned = clean_whois(raw);

        assert!(cleaned.text.contains("Registrar: One, Two"));
        assert!(!cleaned.text.contains("Empty:"));
        assert!(!cleaned.text.contains("NOTICE"));
        assert!(!cleaned.text.contains("After:"));
        assert!(cleaned.text.starts_with("Domain: EXAMPLE.COM"));
        assert!(!cleaned.truncated);
    }

    #[test]
    fn final_output_is_character_limited() {
        let raw = format!("Key: {}", "值".repeat(MAX_DISPLAY_CHARS + 100));
        let cleaned = clean_whois(&raw);
        assert!(cleaned.truncated);
        assert_eq!(cleaned.text.chars().count(), MAX_DISPLAY_CHARS);
    }

    #[test]
    fn detects_no_match_records() {
        assert!(is_not_found("No match for \"IO\"."));
        assert!(is_not_found("NOT FOUND"));
        assert!(!is_not_found("Domain Name: EXAMPLE.COM"));
        assert!(is_not_found(""));
    }

    #[test]
    fn keeps_key_order_and_merges_duplicates() {
        let cleaned = clean_whois("A: 1\nB: 2\nA: 3\n");
        assert_eq!(cleaned.text, "A: 1, 3\nB: 2");
    }

    #[test]
    fn removes_control_characters_from_keys_and_values() {
        let cleaned = clean_whois("Key\u{7}: value\u{7}\n");
        assert_eq!(cleaned.text, "Key: value");
    }
}
