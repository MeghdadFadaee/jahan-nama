use serde_json::{Value, json};

pub fn remaining_label(megabytes: f64) -> String {
    if megabytes.abs() >= 1024.0 {
        format!("{:.2} GB", megabytes / 1024.0)
    } else {
        format!("{megabytes:.2} MB")
    }
}

pub fn remain_json(megabytes: f64) -> Value {
    json!({
        "status": 0,
        "message": "",
        "data": {
            "remain_traffic_mb": megabytes,
            "remain_traffic_label": remaining_label(megabytes),
        },
        "errors": [],
    })
}

pub fn error_json(error: &str) -> Value {
    json!({
        "status": 1,
        "message": error,
        "data": null,
        "errors": ["request_error"],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_mb_and_gb_labels() {
        assert_eq!(remaining_label(512.0), "512.00 MB");
        assert_eq!(remaining_label(1536.0), "1.50 GB");
    }
}
