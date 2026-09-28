use super::*;

pub(crate) fn output<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub(crate) fn parse_json(input: &str) -> Result<Value> {
    serde_json::from_str(input).with_context(|| format!("invalid JSON: {input}"))
}

pub(crate) fn default_asset_dir() -> PathBuf {
    env::var_os("UGC_ASSET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".ugc/assets"))
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
