use serde_json::{Value, json};

pub fn bytes_label(bytes: i64) -> String {
    let bytes_f64 = bytes as f64;
    let kib = 1024.0;
    let mib = kib * 1024.0;
    let gib = mib * 1024.0;

    if bytes_f64 >= gib {
        format!("{:.2} GB", bytes_f64 / gib)
    } else if bytes_f64 >= mib {
        format!("{:.2} MB", bytes_f64 / mib)
    } else if bytes_f64 >= kib {
        format!("{:.2} KB", bytes_f64 / kib)
    } else {
        format!("{bytes} B")
    }
}

pub fn megabytes_label(bytes: i64) -> String {
    format!("{:10.2} MB", bytes as f64 / 1024.0 / 1024.0)
}

pub fn unused_json(values: &[i64]) -> Value {
    let total: i64 = values.iter().sum();
    let packages: Vec<Value> = values
        .iter()
        .map(|value| {
            json!({
                "unused_bit": value,
                "unused_label": bytes_label(*value),
            })
        })
        .collect();

    json!({
        "status": 0,
        "message": "",
        "data": {
            "total_unused_bit": total,
            "total_unused_label": bytes_label(total),
            "packages": packages,
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
