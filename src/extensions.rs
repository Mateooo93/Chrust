//! Extensions from the Chrome Web Store.
//!
//! WebView2 runs extensions that sit unpacked in folders, so the store's own
//! install button cannot drive it. Aurora therefore fetches the package the
//! store serves (a `.crx`: a signed header in front of a zip), unpacks it
//! into `%LOCALAPPDATA%\Aurora\Extensions\<id>`, and lets WebView2 load every
//! folder there. Extensions that rely on Chrome-only APIs load but do
//! nothing visible.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::storage;

/// Chrome extension ids are 32 letters from a to p.
pub fn extension_id(id: &str) -> Option<&str> {
    let id = id.trim();
    (id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))).then_some(id)
}

/// The extension id in a store link, or an id typed on its own.
pub fn id_from_input(input: &str) -> Option<String> {
    let input = input.trim();
    if let Some(id) = extension_id(input) {
        return Some(id.to_string());
    }
    // https://chromewebstore.google.com/detail/<name>/<id>?...
    let url = url::Url::parse(input).ok()?;
    let host = url.host_str()?;
    if host != "chromewebstore.google.com" && host != "chrome.google.com" {
        return None;
    }
    url.path_segments()?.find_map(|part| extension_id(part)).map(str::to_string)
}

/// The extension's page on the Chrome Web Store.
pub fn store_page(id: &str) -> String {
    format!("https://chromewebstore.google.com/detail/{id}")
}

/// Where unpacked extensions live; each extension gets a folder named by id.
pub fn extensions_dir() -> PathBuf {
    storage::data_dir().join("Extensions")
}

fn folder(id: &str) -> PathBuf {
    extensions_dir().join(id)
}

fn agent() -> ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(crate::proxy::agent).clone()
}

/// Downloads the store's current package for `id` and unpacks it, replacing
/// any earlier copy. Returns the extension's name.
pub fn install(id: &str) -> Result<String, String> {
    let id = extension_id(id).ok_or("that is not a Chrome extension id")?;
    // The update service answers with a redirect to the package for the
    // newest version the engine can run.
    let url = format!(
        "https://clients2.google.com/service/update2/crx?response=redirect&acceptformat=crx3\
         &prodversion={}&x=id%3D{id}%26uc",
        browser_version()
    );
    let response = agent().get(&url).call().map_err(|err| match err {
        ureq::Error::Status(404, _) => "The Chrome Web Store does not have that extension.".to_string(),
        ureq::Error::Status(code, _) => format!("The Chrome Web Store answered with error {code}."),
        ureq::Error::Transport(_) => "Could not reach the Chrome Web Store. Check the connection.".to_string(),
    })?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(200 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("The download failed: {err}"))?;
    let zip = crx_payload(&bytes).ok_or("The download is not a Chrome extension package.")?;

    // Unpack into a scratch folder first, so a broken package never damages
    // the copy in use; the swap is a single rename.
    let parent = extensions_dir();
    std::fs::create_dir_all(&parent).map_err(|err| format!("Could not create {}: {err}", parent.display()))?;
    let scratch = parent.join(format!(".{id}.new"));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|err| format!("Could not unpack the extension: {err}"))?;
    if let Err(err) = unpack(zip, &scratch) {
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(err);
    }
    let manifest_text = std::fs::read_to_string(scratch.join("manifest.json"))
        .map_err(|_| "The package has no manifest.json.".to_string())?;
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).map_err(|err| format!("The extension's manifest cannot be read: {err}"))?;
    let name = display_name(&scratch, &manifest).unwrap_or_else(|| id.to_string());

    let target = folder(id);
    let old = parent.join(format!(".{id}.old"));
    let _ = std::fs::remove_dir_all(&old);
    if target.is_dir() && std::fs::rename(&target, &old).is_err() {
        // The running engine may hold it; the new copy replaces it on the
        // next start instead.
        let _ = std::fs::remove_dir_all(&scratch);
        return Err("Close Aurora's other windows and try again.".into());
    }
    if let Err(err) = std::fs::rename(&scratch, &target) {
        let _ = std::fs::rename(&old, &target);
        return Err(format!("Could not install the extension: {err}"));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(name)
}

pub fn uninstall(id: &str) {
    if let Some(id) = extension_id(id) {
        let _ = std::fs::remove_dir_all(folder(id));
    }
}

/// Ids and names of everything in the extensions folder.
pub fn list() -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(extensions_dir()) else { return Vec::new() };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(id) = path.file_name().and_then(|n| n.to_str()).and_then(extension_id) else { continue };
        let name = std::fs::read_to_string(path.join("manifest.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|manifest| display_name(&path, &manifest))
            .unwrap_or_else(|| id.to_string());
        found.push((id.to_string(), name));
    }
    found.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    found
}

/// `__MSG_name__` comes from the extension's own messages file.
fn display_name(folder: &Path, manifest: &serde_json::Value) -> Option<String> {
    let name = manifest["name"].as_str()?;
    let Some(key) = name.strip_prefix("__MSG_").and_then(|k| k.strip_suffix("__")) else {
        return Some(name.to_string());
    };
    let locale = manifest["default_locale"].as_str().unwrap_or("en");
    let text = std::fs::read_to_string(folder.join("_locales").join(locale).join("messages.json")).ok()?;
    let messages: serde_json::Value = serde_json::from_str(&text).ok()?;
    messages.get(key)?["message"].as_str().map(str::to_string)
}

/// The zip inside a CRX package: `Cr24`, a version and a header length, then
/// the signed header and the zip (version 2 keeps two lengths instead).
fn crx_payload(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.len() < 16 || &bytes[..4] != b"Cr24" {
        return None;
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
    match version {
        3 => {
            let header = u32::from_le_bytes(bytes[8..12].try_into().ok()?) as usize;
            bytes.get(12 + header..).filter(|z| z.starts_with(b"PK"))
        }
        2 => {
            let key = u32::from_le_bytes(bytes[8..12].try_into().ok()?) as usize;
            let signature = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as usize;
            bytes.get(16 + key + signature..).filter(|z| z.starts_with(b"PK"))
        }
        _ => None,
    }
}

/// Extracts the zip, refusing entries that would escape `into`.
fn unpack(zip: &[u8], into: &Path) -> Result<(), String> {
    let reader = std::io::Cursor::new(zip);
    let mut archive = zip::ZipArchive::new(reader).map_err(|err| format!("The package cannot be opened: {err}"))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|err| format!("The package is damaged: {err}"))?;
        let Some(relative) = entry.enclosed_name() else {
            return Err("The package tries to write outside its folder.".into());
        };
        let target = into.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&target).map_err(|err| format!("Could not unpack the extension: {err}"))?;
            continue;
        }
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|err| format!("Could not unpack the extension: {err}"))?;
        }
        let mut file =
            std::fs::File::create(&target).map_err(|err| format!("Could not unpack the extension: {err}"))?;
        std::io::copy(&mut entry, &mut file).map_err(|err| format!("Could not unpack the extension: {err}"))?;
    }
    Ok(())
}

/// The Chromium version behind WebView2, so the store picks a package this
/// engine can run.
fn browser_version() -> String {
    wry::webview_version().unwrap_or_else(|_| "120.0.0.0".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ids_from_links_and_text() {
        let id = "ddkjiahejlhfcafbddmgiahcphecmpfh";
        assert_eq!(id_from_input(id).as_deref(), Some(id));
        assert_eq!(
            id_from_input("https://chromewebstore.google.com/detail/ublock-origin-lite/ddkjiahejlhfcafbddmgiahcphecmpfh?hl=en")
                .as_deref(),
            Some(id)
        );
        assert_eq!(id_from_input("https://example.com/detail/x/ddkjiahejlhfcafbddmgiahcphecmpfh"), None);
        assert_eq!(id_from_input("not-an-id"), None);
    }

    #[test]
    fn skips_the_crx_header() {
        let mut bytes = b"Cr24".to_vec();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(b"sig!");
        bytes.extend_from_slice(b"PK\x03\x04zip");
        assert_eq!(crx_payload(&bytes), Some(&b"PK\x03\x04zip"[..]));
        assert_eq!(crx_payload(b"not a crx at all......... "), None);
    }
}
