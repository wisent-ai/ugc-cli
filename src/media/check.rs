use super::*;

pub(crate) fn check(name: &str, passed: bool, passed_message: &str, failed_message: &str) -> QcCheck {
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

pub(crate) fn warning(name: &str, message: &str) -> QcCheck {
    QcCheck {
        name: name.into(),
        status: "WARN".into(),
        message: message.into(),
    }
}

pub(crate) fn reduce_ratio(width: i64, height: i64) -> String {
    let divisor = gcd(width.abs(), height.abs());
    if divisor == 0 {
        return format!("{width}:{height}");
    }
    format!("{}:{}", width / divisor, height / divisor)
}

pub(crate) fn gcd(mut left: i64, mut right: i64) -> i64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

#[cfg(unix)]
pub(crate) fn protect_asset_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        bail!("asset directory must not be a symbolic link");
    }
    const OWNER_ONLY_DIRECTORY_MODE: u32 = 0o700;
    fs::set_permissions(path, fs::Permissions::from_mode(OWNER_ONLY_DIRECTORY_MODE))
        .with_context(|| format!("cannot protect {}", path.display()))
}

#[cfg(not(unix))]
pub(crate) fn protect_asset_directory(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub(crate) fn protect_asset_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    const OWNER_ONLY_FILE_MODE: u32 = 0o600;
    fs::set_permissions(path, fs::Permissions::from_mode(OWNER_ONLY_FILE_MODE))
        .with_context(|| format!("cannot protect {}", path.display()))
}

#[cfg(not(unix))]
pub(crate) fn protect_asset_file(_path: &Path) -> Result<()> {
    Ok(())
}
