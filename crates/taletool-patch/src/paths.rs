use anyhow::{Result, bail};

pub fn normalize_client_path(raw: &str) -> Result<String> {
    let mut path = raw.trim().replace('\\', "/");

    if let Some(rest) = path.strip_prefix("$(INSTALLED)/") {
        path = rest.to_string();
    }

    while let Some(rest) = path.strip_prefix('/') {
        path = rest.to_string();
    }

    if path.is_empty() {
        bail!("empty client path");
    }

    if path.contains(':') {
        bail!("client path must not contain a Windows drive or URI scheme: {raw}");
    }

    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => bail!("client path must not contain parent traversal: {raw}"),
            clean => parts.push(clean),
        }
    }

    if parts.is_empty() {
        bail!("empty client path");
    }

    Ok(parts.join("/"))
}
