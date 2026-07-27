use std::{
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use chrono::Duration;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    db::Store,
    model::{Asset, QcCheck, QcReport, Submission},
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QcPolicy {
    pub expected_mime_prefix: Option<String>,
    pub min_duration_ms: Option<i64>,
    pub max_duration_ms: Option<i64>,
    pub allowed_aspect_ratios: Vec<String>,
    pub max_bytes: Option<i64>,
}

pub fn import_asset(
    store: &Store,
    asset_dir: &Path,
    source: &Path,
    submission_id: Option<&str>,
    role: &str,
    source_url: Option<String>,
    actor: &str,
) -> Result<Asset> {
    if !source.is_file() {
        bail!("asset source is not a file: {}", source.display());
    }
    if let Some(submission) = submission_id {
        let _: Submission = store.get("submission", submission)?;
    }
    fs::create_dir_all(asset_dir)
        .with_context(|| format!("cannot create {}", asset_dir.display()))?;
    let temp_path = asset_dir.join(format!("{}.part", Store::id()));
    let input = File::open(source)?;
    let mut reader = BufReader::new(input);
    let output = File::create(&temp_path)?;
    let mut writer = BufWriter::new(output);
    let mut hasher = Sha256::new();
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            break;
        }
        hasher.update(buffer);
        writer.write_all(buffer)?;
        let consumed = buffer.len();
        reader.consume(consumed);
    }
    writer.flush()?;
    drop(writer);

    let sha256 = hex::encode(hasher.finalize());
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| safe_extension(value));
    let file_name = match extension {
        Some(value) => format!("{sha256}.{value}"),
        None => sha256.clone(),
    };
    let final_path = asset_dir.join(file_name);
    if final_path.exists() {
        fs::remove_file(&temp_path)?;
    } else {
        fs::rename(&temp_path, &final_path)?;
    }

    let file_meta = fs::metadata(&final_path)?;
    let mime_type = infer::get_from_path(&final_path)?
        .map(|kind| kind.mime_type().to_string())
        .unwrap_or_else(|| "application/octet-stream".into());
    let probe = probe_media(&final_path)
        .unwrap_or_else(|error| json!({"probe_warning": error.to_string()}));
    let duration_ms = probe.get("duration_ms").and_then(Value::as_i64);
    let width = probe.get("width").and_then(Value::as_i64);
    let height = probe.get("height").and_then(Value::as_i64);
    let asset = Asset {
        id: Store::id(),
        submission_id: submission_id.map(str::to_owned),
        role: role.into(),
        source_url,
        local_path: final_path.to_string_lossy().into_owned(),
        sha256: sha256.clone(),
        mime_type,
        bytes: i64::try_from(file_meta.len()).context("asset size exceeds supported range")?,
        duration_ms,
        width,
        height,
        metadata: probe,
        created_at: Store::now(),
    };
    store.put(
        "asset",
        &asset.id,
        submission_id,
        None,
        "available",
        Some(&sha256),
        &asset,
        &asset.created_at,
    )?;
    store.audit(
        "asset",
        &asset.id,
        "imported",
        actor,
        &json!({"source": source, "submission_id": submission_id}),
    )?;
    Ok(asset)
}

pub fn run_qc(store: &Store, asset_id: &str, policy: &QcPolicy, actor: &str) -> Result<QcReport> {
    let asset: Asset = store.get("asset", asset_id)?;
    let mut checks = Vec::new();
    let path = PathBuf::from(&asset.local_path);
    checks.push(check(
        "file_exists",
        path.is_file(),
        "asset file is present",
        "asset file is missing",
    ));
    checks.push(check(
        "hash_present",
        !asset.sha256.is_empty(),
        "SHA-256 is recorded",
        "SHA-256 is missing",
    ));

    if let Some(prefix) = &policy.expected_mime_prefix {
        checks.push(check(
            "mime_type",
            asset.mime_type.starts_with(prefix),
            &format!("MIME type {} is accepted", asset.mime_type),
            &format!("MIME type {} does not start with {prefix}", asset.mime_type),
        ));
    }
    if let Some(max_bytes) = policy.max_bytes {
        checks.push(check(
            "file_size",
            asset.bytes <= max_bytes,
            &format!("file size {} is within limit", asset.bytes),
            &format!("file size {} exceeds {max_bytes}", asset.bytes),
        ));
    }
    if let Some(minimum) = policy.min_duration_ms {
        match asset.duration_ms {
            Some(duration) => checks.push(check(
                "minimum_duration",
                duration >= minimum,
                &format!("duration {duration}ms meets minimum"),
                &format!("duration {duration}ms is below {minimum}ms"),
            )),
            None => checks.push(warning("minimum_duration", "duration is unavailable")),
        }
    }
    if let Some(maximum) = policy.max_duration_ms {
        match asset.duration_ms {
            Some(duration) => checks.push(check(
                "maximum_duration",
                duration <= maximum,
                &format!("duration {duration}ms meets maximum"),
                &format!("duration {duration}ms exceeds {maximum}ms"),
            )),
            None => checks.push(warning("maximum_duration", "duration is unavailable")),
        }
    }
    if !policy.allowed_aspect_ratios.is_empty() {
        match (asset.width, asset.height) {
            (Some(width), Some(height)) => {
                let actual = reduce_ratio(width, height);
                checks.push(check(
                    "aspect_ratio",
                    policy
                        .allowed_aspect_ratios
                        .iter()
                        .any(|candidate| candidate == &actual),
                    &format!("aspect ratio {actual} is accepted"),
                    &format!(
                        "aspect ratio {actual} is not in {}",
                        policy.allowed_aspect_ratios.join(",")
                    ),
                ));
            }
            _ => checks.push(warning("aspect_ratio", "dimensions are unavailable")),
        }
    }

    let status = if checks.iter().any(|item| item.status == "FAIL") {
        "FAIL"
    } else if checks.iter().any(|item| item.status == "WARN") {
        "WARN"
    } else {
        "PASS"
    };
    let report = QcReport {
        status: status.into(),
        checks,
    };
    store.audit(
        "asset",
        asset_id,
        "qc_completed",
        actor,
        &serde_json::to_value(&report)?,
    )?;

    if let Some(submission_id) = &asset.submission_id {
        let mut submission: Submission = store.get("submission", submission_id)?;
        submission.qc_status = Some(report.status.clone());
        submission.qc_report = Some(serde_json::to_value(&report)?);
        if matches!(
            submission.status.as_str(),
            "received" | "ingesting" | "qc_pending"
        ) {
            submission.status = "pending_review".into();
        }
        store.put(
            "submission",
            &submission.id,
            Some(&submission.assignment_id),
            None,
            &submission.status,
            submission.external_submission_id.as_deref(),
            &submission,
            &submission.submitted_at,
        )?;
    }
    Ok(report)
}

fn probe_media(path: &Path) -> Result<Value> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type,width,height",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .context("ffprobe is not installed")?;
    if !output.status.success() {
        bail!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let raw: Value = serde_json::from_slice(&output.stdout)?;
    let seconds = raw
        .pointer("/format/duration")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<f64>().ok());
    let millis = seconds.map(|value| {
        (value * Duration::seconds("s".len() as i64).num_milliseconds() as f64).round() as i64
    });
    let video_stream = raw
        .get("streams")
        .and_then(Value::as_array)
        .and_then(|streams| {
            streams
                .iter()
                .find(|stream| stream.get("codec_type").and_then(Value::as_str) == Some("video"))
        });
    Ok(json!({
        "duration_ms": millis,
        "width": video_stream.and_then(|stream| stream.get("width")).and_then(Value::as_i64),
        "height": video_stream.and_then(|stream| stream.get("height")).and_then(Value::as_i64),
        "ffprobe": raw,
    }))
}

fn safe_extension(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

fn check(name: &str, passed: bool, passed_message: &str, failed_message: &str) -> QcCheck {
    QcCheck {
        name: name.into(),
        status: if passed { "PASS".into() } else { "FAIL".into() },
        message: if passed {
            passed_message.into()
        } else {
            failed_message.into()
        },
    }
}

fn warning(name: &str, message: &str) -> QcCheck {
    QcCheck {
        name: name.into(),
        status: "WARN".into(),
        message: message.into(),
    }
}

fn reduce_ratio(width: i64, height: i64) -> String {
    let divisor = gcd(width.abs(), height.abs());
    if divisor == "".len() as i64 {
        return format!("{width}:{height}");
    }
    format!("{}:{}", width / divisor, height / divisor)
}

fn gcd(mut left: i64, mut right: i64) -> i64 {
    while right != "".len() as i64 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}
