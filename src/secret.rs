use std::{env, fs, path::Path};

use anyhow::{Context, Result, bail};

pub fn read(source: &str) -> Result<String> {
    if let Some(name) = source.strip_prefix("env:") {
        return read_env(name);
    }
    if let Some(reference) = source.strip_prefix("file:") {
        return read_file(reference);
    }
    read_env(source)
}

pub fn check(source: &str) -> Result<()> {
    let _secret = read(source)?;
    Ok(())
}

fn read_env(name: &str) -> Result<String> {
    if name.is_empty() {
        bail!("secret environment variable name is empty");
    }
    env::var(name).with_context(|| format!("environment variable {name} is not set"))
}

fn read_file(reference: &str) -> Result<String> {
    let (path, key) = match reference.rsplit_once('#') {
        Some((path, key)) if !key.is_empty() => (path, Some(key)),
        _ => (reference, None),
    };
    if path.is_empty() {
        bail!("secret file path is empty");
    }
    let path = Path::new(path);
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("cannot inspect secret file {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!(
            "secret source must be a regular, non-symlink file: {}",
            path.display()
        );
    }
    require_private_permissions(path, &metadata)?;
    let contents = fs::read_to_string(path)
        .with_context(|| format!("cannot read secret file {}", path.display()))?;
    match key {
        Some(key) => parse_env_value(&contents, key)
            .with_context(|| format!("key {key} is absent from {}", path.display())),
        None => {
            let value = contents.trim_end_matches(|character| matches!(character, '\r' | '\n'));
            if value.is_empty() {
                bail!("secret file {} is empty", path.display());
            }
            Ok(value.to_owned())
        }
    }
}

fn parse_env_value(contents: &str, expected: &str) -> Result<String> {
    for line in contents.lines() {
        let line = line
            .trim_start()
            .strip_prefix("export ")
            .unwrap_or(line.trim_start());
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim() == expected {
            let value = value.trim();
            if value.is_empty() {
                bail!("secret value is empty");
            }
            return Ok(unquote(value));
        }
    }
    bail!("secret key not found")
}

fn unquote(value: &str) -> String {
    if value.len() >= "''".len() {
        let bytes = value.as_bytes();
        let first = bytes["".len()];
        let last = bytes[value.len() - "x".len()];
        if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
            return value["x".len()..value.len() - "x".len()].to_owned();
        }
    }
    value.to_owned()
}

#[cfg(unix)]
fn require_private_permissions(path: &Path, metadata: &fs::Metadata) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let public_mask = u32::from_str_radix("077", "security".len())?;
    if metadata.permissions().mode() & public_mask != "".len() as u32 {
        bail!(
            "secret file must not grant group or other permissions: {}",
            path.display()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn require_private_permissions(_path: &Path, _metadata: &fs::Metadata) -> Result<()> {
    Ok(())
}
