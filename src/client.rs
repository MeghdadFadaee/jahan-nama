use std::fmt;
use std::path::Path;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;

use crate::DotEnvStore;

pub type Result<T> = std::result::Result<T, JahanNamaError>;

#[derive(Debug)]
pub enum JahanNamaError {
    Io(std::io::Error),
    Http(reqwest::Error),
    MissingEnv(String),
    UnexpectedResponse(&'static str),
    Auth(&'static str),
    Json(String),
    Gui(String),
}

impl fmt::Display for JahanNamaError {
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

impl std::error::Error for JahanNamaError {}

impl From<std::io::Error> for JahanNamaError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<reqwest::Error> for JahanNamaError {
    fn from(value: reqwest::Error) -> Self {
        Self::Http(value)
    }
}

#[derive(Debug)]
pub struct JahanNamaClient {
    env: DotEnvStore,
    http: Client,
    username: String,
    password: String,
    token: Option<String>,
}

impl JahanNamaClient {
    pub const AUTH_URL: &'static str = "https://qomservice.webotel.ir/api/login/AuthenticateWeb";
    pub const REMAIN_URL: &'static str = "https://qomservice.webotel.ir/api/BaseInfo/GetUserRemain";

    pub fn new(env_path: impl AsRef<Path>) -> Result<Self> {
        let env = DotEnvStore::new(env_path)?;
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .default_headers(default_headers())
            .build()?;

        let username = required_env(&env, "JAHAN_NAMA_USERNAME")?;
        let password = required_env(&env, "JAHAN_NAMA_PASSWORD")?;
        let token = env
            .get("JAHAN_NAMA_TOKEN")
            .filter(|value| !value.is_empty())
            .map(str::to_owned);

        Ok(Self {
            env,
            http,
            username,
            password,
            token,
        })
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    pub fn reset_auth(&mut self) -> Result<()> {
        self.token = None;
        clear_auth_values(&mut self.env);
        self.env.save()
    }

    pub fn ensure_token(&mut self, force_login: bool) -> Result<String> {
        if !force_login && let Some(token) = &self.token {
            return Ok(token.clone());
        }

        self.login()?;
        self.token.clone().ok_or(JahanNamaError::Auth(
            "Authentication failed: token not available.",
        ))
    }

    pub fn login(&mut self) -> Result<Value> {
        let body = [
            ("Username", self.username.as_str()),
            ("Password", self.password.as_str()),
            ("DeviceTypeEnum", "4"),
            ("IP", ""),
        ];

        let payload: Value = self
            .http
            .post(Self::AUTH_URL)
            .form(&body)
            .send()?
            .error_for_status()?
            .json()?;

        if !payload.is_object() {
            return Err(JahanNamaError::UnexpectedResponse(
                "Unexpected authentication response format.",
            ));
        }

        let Some(token) = string_field(&payload, "Token") else {
            return Err(JahanNamaError::Auth(
                "Authentication failed: token not available.",
            ));
        };

        self.token = Some(token);
        self.save_auth_to_env()?;
        Ok(payload)
    }

    pub fn get_remaining_traffic_mb(&mut self) -> Result<f64> {
        let payload = self.get_remain_response()?;
        remain_traffic_mb(&payload).ok_or(JahanNamaError::UnexpectedResponse(
            "Unexpected remaining traffic response format.",
        ))
    }

    pub fn get_remain_response(&mut self) -> Result<Value> {
        let had_cached_token = self.token.is_some();
        let token = self.ensure_token(false)?;

        match self.get_remain_response_with_token(&token) {
            Ok(payload) if remain_traffic_mb(&payload).is_some() => Ok(payload),
            Ok(_) if had_cached_token => self.login_and_get_remain_response(),
            Ok(_) => Err(JahanNamaError::UnexpectedResponse(
                "Unexpected remaining traffic response format.",
            )),
            Err(JahanNamaError::Http(error))
                if had_cached_token && is_auth_status(error.status()) =>
            {
                self.login_and_get_remain_response()
            }
            Err(error) => Err(error),
        }
    }

    fn login_and_get_remain_response(&mut self) -> Result<Value> {
        let token = self.ensure_token(true)?;
        let payload = self.get_remain_response_with_token(&token)?;
        if remain_traffic_mb(&payload).is_none() {
            return Err(JahanNamaError::UnexpectedResponse(
                "Unexpected remaining traffic response format.",
            ));
        }
        Ok(payload)
    }

    fn get_remain_response_with_token(&self, token: &str) -> Result<Value> {
        let payload: Value = self
            .http
            .get(Self::REMAIN_URL)
            .query(&[("Token", token)])
            .send()?
            .error_for_status()?
            .json()?;

        if !payload.is_object() {
            return Err(JahanNamaError::UnexpectedResponse(
                "Unexpected remaining traffic response format.",
            ));
        }

        Ok(payload)
    }

    fn save_auth_to_env(&mut self) -> Result<()> {
        self.env
            .set("JAHAN_NAMA_TOKEN", self.token.clone().unwrap_or_default());
        self.env.save()
    }
}

pub fn reset_saved_token(env_path: impl AsRef<Path>) -> Result<()> {
    let mut env = DotEnvStore::new(env_path)?;
    clear_auth_values(&mut env);
    env.save()
}

pub fn remain_traffic_mb(data: &Value) -> Option<f64> {
    data.get("RemainTraffic").and_then(value_to_f64)
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
    headers
}

fn required_env(env: &DotEnvStore, key: &str) -> Result<String> {
    env.get(key)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| JahanNamaError::MissingEnv(key.to_owned()))
}

fn clear_auth_values(env: &mut DotEnvStore) {
    env.set("JAHAN_NAMA_TOKEN", "");
}

fn is_auth_status(status: Option<StatusCode>) -> bool {
    matches!(
        status,
        Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
    )
}

fn string_field(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn value_to_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Bool(_) | Value::Null | Value::Array(_) | Value::Object(_) => None,
        Value::Number(number) => number.as_f64(),
        Value::String(value) => value.trim().parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn extracts_numeric_remain_traffic() {
        let payload = json!({
            "RemainTraffic": 1536,
        });

        assert_eq!(remain_traffic_mb(&payload), Some(1536.0));
    }

    #[test]
    fn extracts_string_remain_traffic() {
        let payload = json!({
            "RemainTraffic": "2048.5",
        });

        assert_eq!(remain_traffic_mb(&payload), Some(2048.5));
    }

    #[test]
    fn ignores_invalid_remain_traffic() {
        let payload = json!({
            "RemainTraffic": true,
        });

        assert_eq!(remain_traffic_mb(&payload), None);
    }
}
