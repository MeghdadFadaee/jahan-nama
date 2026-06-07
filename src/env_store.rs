use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::Result;

#[derive(Debug, Clone)]
pub struct DotEnvStore {
    path: PathBuf,
    lines: Vec<String>,
    data: BTreeMap<String, String>,
}

impl DotEnvStore {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut store = Self {
            path,
            lines: Vec::new(),
            data: BTreeMap::new(),
        };
        store.load()?;
        Ok(store)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.data.get(key).map(String::as_str)
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        self.data.insert(key.to_owned(), value.into());
    }

    pub fn save(&self) -> Result<()> {
        let mut existing_keys = HashSet::new();
        let mut new_lines = Vec::with_capacity(self.lines.len() + self.data.len());

        for line in &self.lines {
            match parse_key(line) {
                Some(key) if self.data.contains_key(key) => {
                    let value = self.data.get(key).expect("checked above");
                    new_lines.push(format!("{key}={}", quote(value)));
                    existing_keys.insert(key.to_owned());
                }
                _ => new_lines.push(line.clone()),
            }
        }

        for (key, value) in &self.data {
            if !existing_keys.contains(key) {
                new_lines.push(format!("{key}={}", quote(value)));
            }
        }

        let mut body = new_lines.join("\n");
        body.push('\n');
        fs::write(&self.path, body)?;
        Ok(())
    }

    fn load(&mut self) -> Result<()> {
        if !self.path.exists() {
            return Ok(());
        }

        let body = fs::read_to_string(&self.path)?;
        self.lines = body.lines().map(str::to_owned).collect();

        for line in &self.lines {
            let Some(key) = parse_key(line) else {
                continue;
            };
            let Some((_, value)) = line.split_once('=') else {
                continue;
            };
            self.data
                .insert(key.to_owned(), unquote(value.trim()).to_owned());
        }

        Ok(())
    }
}

fn parse_key(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    if is_valid_key(key) { Some(key) } else { None }
}

fn is_valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() < 2 || bytes.first() != bytes.last() {
        return value.to_owned();
    }

    let quote = bytes[0];
    if quote != b'"' && quote != b'\'' {
        return value.to_owned();
    }

    let inner = &value[1..value.len() - 1];
    let mut result = String::with_capacity(inner.len());
    let mut escaped = false;

    for ch in inner.chars() {
        if escaped {
            result.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            result.push(ch);
        }
    }

    if escaped {
        result.push('\\');
    }

    result
}

fn quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for ch in value.chars() {
        if ch == '\\' || ch == '"' {
            quoted.push('\\');
        }
        quoted.push(ch);
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn saves_existing_lines_and_appends_new_keys() {
        let path = temp_env_path();
        fs::write(
            &path,
            "# comment\nMCI_USERNAME=\"912\"\n\nMCI_ACCESS_TOKEN=\"old\"\n",
        )
        .unwrap();

        let mut env = DotEnvStore::new(&path).unwrap();
        env.set("MCI_ACCESS_TOKEN", "new token");
        env.set("MCI_REFRESH_TOKEN", "refresh");
        env.save().unwrap();

        let saved = fs::read_to_string(&path).unwrap();
        assert!(saved.contains("# comment\n"));
        assert!(saved.contains("MCI_USERNAME=\"912\""));
        assert!(saved.contains("MCI_ACCESS_TOKEN=\"new token\""));
        assert!(saved.contains("MCI_REFRESH_TOKEN=\"refresh\""));

        fs::remove_file(path).unwrap();
    }

    fn temp_env_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("mci-client-test-{nanos}.env"))
    }
}
