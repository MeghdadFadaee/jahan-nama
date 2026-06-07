use std::fmt;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{Value, json};

use crate::DotEnvStore;

pub type Result<T> = std::result::Result<T, MciError>;

#[derive(Debug)]
pub enum MciError {
    Io(std::io::Error),
    Http(reqwest::Error),
    MissingEnv(String),
    UnexpectedResponse(&'static str),
    Auth(&'static str),
    Json(String),
    Gui(String),
}

impl fmt::Display for MciError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Http(error) => write!(f, "{error}"),
            Self::MissingEnv(key) => write!(f, "Missing required environment variable: {key}"),
            Self::UnexpectedResponse(message) => write!(f, "{message}"),
            Self::Auth(message) => write!(f, "{message}"),
            Self::Json(message) => write!(f, "{message}"),
            Self::Gui(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for MciError {}

impl From<std::io::Error> for MciError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<reqwest::Error> for MciError {
    fn from(value: reqwest::Error) -> Self {
        Self::Http(value)
    }
}

#[derive(Debug)]
pub struct MciInternetClient {
    env: DotEnvStore,
    http: Client,
    username: String,
    password: String,
    access_token: Option<String>,
    refresh_token: Option<String>,
    session_state: Option<String>,
    access_token_expires_at: Option<i64>,
    refresh_token_expires_at: Option<i64>,
}

impl MciInternetClient {
    pub const AUTH_URL: &'static str = "https://my.mci.ir/api/idm/v1/auth";
    pub const PACKAGES_URL: &'static str = "https://my.mci.ir/api/unit/v1/packages/details";

    pub fn new(env_path: impl AsRef<Path>) -> Result<Self> {
        let env = DotEnvStore::new(env_path)?;
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .default_headers(default_headers())
            .build()?;

        let username = required_env(&env, "MCI_USERNAME")?;
        let password = required_env(&env, "MCI_PASSWORD")?;

        let access_token = env
            .get("MCI_ACCESS_TOKEN")
            .filter(|v| !v.is_empty())
            .map(str::to_owned);
        let refresh_token = env
            .get("MCI_REFRESH_TOKEN")
            .filter(|v| !v.is_empty())
            .map(str::to_owned);
        let session_state = env
            .get("MCI_SESSION_STATE")
            .filter(|v| !v.is_empty())
            .map(str::to_owned);
        let access_token_expires_at = safe_i64(env.get("MCI_ACCESS_TOKEN_EXPIRES_AT"));
        let refresh_token_expires_at = safe_i64(env.get("MCI_REFRESH_TOKEN_EXPIRES_AT"));

        Ok(Self {
            env,
            http,
            username,
            password,
            access_token,
            refresh_token,
            session_state,
            access_token_expires_at,
            refresh_token_expires_at,
        })
    }

    pub fn access_token(&self) -> Option<&str> {
        self.access_token.as_deref()
    }

    pub fn refresh_token(&self) -> Option<&str> {
        self.refresh_token.as_deref()
    }

    pub fn reset_auth(&mut self) -> Result<()> {
        self.access_token = None;
        self.refresh_token = None;
        self.session_state = None;
        self.access_token_expires_at = None;
        self.refresh_token_expires_at = None;

        clear_auth_values(&mut self.env);
        self.env.save()
    }

    pub fn ensure_token(&mut self, force_refresh: bool) -> Result<String> {
        if !force_refresh
            && self
                .access_token
                .as_ref()
                .is_some_and(|_| token_is_valid(self.access_token_expires_at))
        {
            return Ok(self.access_token.clone().expect("checked above"));
        }

        if self.refresh_token.is_some()
            && (force_refresh
                || token_is_valid(self.refresh_token_expires_at)
                || self.access_token.is_some())
        {
            match self.refresh() {
                Ok(_) if self.access_token.is_some() => {
                    return Ok(self.access_token.clone().expect("checked above"));
                }
                Ok(_) => {}
                Err(MciError::Http(error)) if error.status().is_some() => {}
                Err(error) => return Err(error),
            }
        }

        self.login()?;
        self.access_token.clone().ok_or(MciError::Auth(
            "Authentication failed: access token not available.",
        ))
    }

    pub fn login(&mut self) -> Result<Value> {
        let payload = self.auth_request(
            json!({
                "username": self.username,
                "credential": self.password,
                "credential_type": "PASSWORD",
            }),
            None,
        )?;
        self.save_auth_to_env(&payload)?;
        Ok(payload)
    }

    pub fn refresh(&mut self) -> Result<Value> {
        let Some(refresh_token) = self.refresh_token.clone() else {
            return self.login();
        };

        let payload = self.auth_request(
            json!({
                "username": self.username,
                "credential_type": "REFRESH_TOKEN",
                "credential": refresh_token,
            }),
            self.access_token.as_deref(),
        )?;
        self.save_auth_to_env(&payload)?;
        Ok(payload)
    }

    pub fn get_unused_amounts_bytes(&mut self) -> Result<Vec<i64>> {
        let payload = self.get_packages_response()?;
        Ok(collect_unused_amounts(&payload))
    }

    pub fn get_packages_response(&mut self) -> Result<Value> {
        let mut token = self.ensure_token(false)?;
        let mut response = self
            .http
            .get(Self::PACKAGES_URL)
            .bearer_auth(&token)
            .send()?;

        if response.status() == StatusCode::UNAUTHORIZED {
            token = self.ensure_token(true)?;
            response = self
                .http
                .get(Self::PACKAGES_URL)
                .bearer_auth(&token)
                .send()?;
        }

        let payload: Value = response.error_for_status()?.json()?;
        if !payload.is_object() {
            return Err(MciError::UnexpectedResponse(
                "Unexpected packages response format.",
            ));
        }

        Ok(payload)
    }

    fn auth_request(&self, body: Value, bearer_token: Option<&str>) -> Result<Value> {
        let mut request = self.http.post(Self::AUTH_URL).json(&body);
        if let Some(token) = bearer_token {
            request = request.bearer_auth(token);
        }

        let payload: Value = request.send()?.error_for_status()?.json()?;
        if !payload.is_object() {
            return Err(MciError::UnexpectedResponse(
                "Unexpected auth response format.",
            ));
        }

        Ok(payload)
    }

    fn save_auth_to_env(&mut self, payload: &Value) -> Result<()> {
        if let Some(value) = string_field(payload, "access_token") {
            self.access_token = Some(value);
        }
        if let Some(value) = string_field(payload, "refresh_token") {
            self.refresh_token = Some(value);
        }
        if let Some(value) = string_field(payload, "session_state") {
            self.session_state = Some(value);
        }

        if let Some(expires_at) = expiry_from_seconds(payload.get("expires_in")) {
            self.access_token_expires_at = Some(expires_at);
        }
        if let Some(expires_at) = expiry_from_seconds(payload.get("refresh_expires_in")) {
            self.refresh_token_expires_at = Some(expires_at);
        }

        self.env.set(
            "MCI_ACCESS_TOKEN",
            self.access_token.clone().unwrap_or_default(),
        );
        self.env.set(
            "MCI_REFRESH_TOKEN",
            self.refresh_token.clone().unwrap_or_default(),
        );
        self.env.set(
            "MCI_SESSION_STATE",
            self.session_state.clone().unwrap_or_default(),
        );
        self.env.set(
            "MCI_ACCESS_TOKEN_EXPIRES_AT",
            self.access_token_expires_at
                .map(|value| value.to_string())
                .unwrap_or_default(),
        );
        self.env.set(
            "MCI_REFRESH_TOKEN_EXPIRES_AT",
            self.refresh_token_expires_at
                .map(|value| value.to_string())
                .unwrap_or_default(),
        );
        self.env.save()
    }
}

pub fn reset_saved_auth(env_path: impl AsRef<Path>) -> Result<()> {
    let mut env = DotEnvStore::new(env_path)?;
    clear_auth_values(&mut env);
    env.save()
}

pub fn collect_unused_amounts(data: &Value) -> Vec<i64> {
    let mut results = Vec::new();
    collect_unused_amounts_inner(data, &mut results);
    results
}

fn collect_unused_amounts_inner(data: &Value, results: &mut Vec<i64>) {
    match data {
        Value::Object(map) => {
            for (key, value) in map {
                if key == "unusedAmount"
                    && let Some(parsed) = value_to_i64(value)
                {
                    results.push(parsed);
                }
                collect_unused_amounts_inner(value, results);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_unused_amounts_inner(item, results);
            }
        }
        _ => {}
    }
}

fn default_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("user-agent"),
        HeaderValue::from_static("Mozilla/5.0"),
    );
    headers.insert(
        HeaderName::from_static("accept"),
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        HeaderName::from_static("origin"),
        HeaderValue::from_static("https://my.mci.ir"),
    );
    headers.insert(
        HeaderName::from_static("referer"),
        HeaderValue::from_static("https://my.mci.ir/"),
    );
    headers.insert(
        HeaderName::from_static("platform"),
        HeaderValue::from_static("WEB"),
    );
    headers.insert(
        HeaderName::from_static("version"),
        HeaderValue::from_static("1.29.0"),
    );
    headers
}

fn required_env(env: &DotEnvStore, key: &str) -> Result<String> {
    env.get(key)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| MciError::MissingEnv(key.to_owned()))
}

fn clear_auth_values(env: &mut DotEnvStore) {
    env.set("MCI_ACCESS_TOKEN", "");
    env.set("MCI_REFRESH_TOKEN", "");
    env.set("MCI_SESSION_STATE", "");
    env.set("MCI_ACCESS_TOKEN_EXPIRES_AT", "");
    env.set("MCI_REFRESH_TOKEN_EXPIRES_AT", "");
}

fn safe_i64(value: Option<&str>) -> Option<i64> {
    value.and_then(|value| {
        if value.is_empty() {
            None
        } else {
            value.parse().ok()
        }
    })
}

fn string_field(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn expiry_from_seconds(expires_in: Option<&Value>) -> Option<i64> {
    value_to_i64(expires_in?).map(|seconds| now_unix() + seconds - 30)
}

fn token_is_valid(expires_at: Option<i64>) -> bool {
    expires_at.is_some_and(|value| value > now_unix())
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn value_to_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Bool(_) | Value::Null | Value::Array(_) | Value::Object(_) => None,
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64)),
        Value::String(value) => {
            let value = value.trim();
            if value.contains('.') {
                value.parse::<f64>().ok().map(|value| value as i64)
            } else {
                value.parse().ok()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn collects_unused_amounts_recursively() {
        let payload = json!({
            "packageItems": [
                {
                    "unusedAmount": 100,
                    "nested": [{"unusedAmount": "200.8"}, {"unusedAmount": true}]
                }
            ],
            "unusedAmount": "300"
        });

        assert_eq!(collect_unused_amounts(&payload), vec![100, 200, 300]);
    }
}
