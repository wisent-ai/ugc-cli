use super::*;

/// Whether this invocation asked for `--text`; set once, before any command
/// prints.
static TEXT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub(crate) fn set_text(text: bool) {
    let _ = TEXT.set(text);
}

/// One answer: pretty JSON for machines, or with `--text` the same document
/// as indented `path: value` lines, list entries as `- ` lines (cli.md rule 13).
pub(crate) fn output<T: Serialize>(value: &T) -> Result<()> {
    if !TEXT.get().copied().unwrap_or(false) {
        println!("{}", serde_json::to_string_pretty(value)?);
        return Ok(());
    }
    let mut rendered = String::new();
    render(&serde_json::to_value(value)?, 0, &mut rendered);
    print!("{rendered}");
    Ok(())
}

fn render(value: &Value, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    let scalar = |value: &Value| match value {
        Value::Null => Some("none".to_string()),
        Value::String(text) => Some(text.clone()),
        Value::Bool(_) | Value::Number(_) => Some(value.to_string()),
        Value::Array(items) if items.is_empty() => Some("(none)".to_string()),
        Value::Object(fields) if fields.is_empty() => Some("(none)".to_string()),
        Value::Array(_) | Value::Object(_) => None,
    };
    match value {
        Value::Object(fields) => {
            for (key, field) in fields {
                match scalar(field) {
                    Some(text) => out.push_str(&format!("{pad}{key}: {text}\n")),
                    None => {
                        out.push_str(&format!("{pad}{key}:\n"));
                        render(field, depth + 1, out);
                    }
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                match scalar(item) {
                    Some(text) => out.push_str(&format!("{pad}- {text}\n")),
                    None => {
                        out.push_str(&format!("{pad}-\n"));
                        render(item, depth + 1, out);
                    }
                }
            }
        }
        other => out.push_str(&format!("{pad}{other}\n")),
    }
}

pub(crate) fn parse_json(input: &str) -> Result<Value> {
    serde_json::from_str(input).with_context(|| format!("invalid JSON: {input}"))
}

/// The private asset directory: `--asset-dir`, else `UGC_ASSET_DIR`. No
/// directory is assumed; a missing one is refused by name.
pub(crate) fn asset_dir(option: Option<PathBuf>) -> Result<PathBuf> {
    option
        .or_else(|| env::var_os("UGC_ASSET_DIR").map(PathBuf::from))
        .context("name the private asset directory with --asset-dir or UGC_ASSET_DIR")
}

pub(crate) fn option_or_env(option: Option<String>, name: &str) -> Result<String> {
    option
        .or_else(|| env::var(name).ok())
        .with_context(|| format!("provide the option or set {name}"))
}

pub(crate) fn first_option_or_env(option: Option<String>, names: &[&str]) -> Result<String> {
    option
        .or_else(|| names.iter().find_map(|name| env::var(name).ok()))
        .with_context(|| format!("provide --base-url or set {}", names.join(" or ")))
}

pub(crate) fn read_input(path: Option<&Path>) -> Result<Vec<u8>> {
    match path {
        Some(path) => fs::read(path).with_context(|| format!("cannot read {}", path.display())),
        None => {
            let mut body = Vec::new();
            io::stdin().read_to_end(&mut body)?;
            Ok(body)
        }
    }
}

pub(crate) fn reject_symbolic_link_output(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("standalone export path must not be a symbolic link")
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("cannot inspect {}", path.display())),
    }
}

#[cfg(unix)]
pub(crate) fn protect_private_output(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    const OWNER_ONLY_FILE_MODE: u32 = 0o600;
    fs::set_permissions(path, fs::Permissions::from_mode(OWNER_ONLY_FILE_MODE))
        .with_context(|| format!("cannot protect {}", path.display()))
}

#[cfg(not(unix))]
pub(crate) fn protect_private_output(_path: &Path) -> Result<()> {
    Ok(())
}
