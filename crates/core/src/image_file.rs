//! Read a local image file the transcript references, for an inline preview.

use std::path::{Path, PathBuf};

use base64::Engine;

use crate::session::render::RenderedImage;

/// Larger files are refused rather than base64-encoded into the webview.
pub const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

fn media_type(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        "heic" => "image/heic",
        _ => return None,
    })
}

/// Read `path` (absolute, or `~/…` against the user's home) as an image.
pub fn read_image_file(path: &str) -> Result<RenderedImage, String> {
    read_image_at(path, dirs::home_dir().as_deref(), MAX_IMAGE_BYTES)
}

/// Errors are `not_image`, `not_found` or `too_large`. The extension is
/// checked again after canonicalising so a symlink can't reach a non-image.
pub fn read_image_at(path: &str, home: Option<&Path>, cap: u64) -> Result<RenderedImage, String> {
    let p: PathBuf = match path.strip_prefix("~/") {
        Some(rest) => home.ok_or("not_found")?.join(rest),
        None => PathBuf::from(path),
    };
    if !p.is_absolute() {
        return Err("not_found".into());
    }
    media_type(&p).ok_or("not_image")?;
    let real = p.canonicalize().map_err(|_| "not_found")?;
    let mt = media_type(&real).ok_or("not_image")?;
    let meta = std::fs::metadata(&real).map_err(|_| "not_found")?;
    if !meta.is_file() {
        return Err("not_found".into());
    }
    if meta.len() > cap {
        return Err("too_large".into());
    }
    let bytes = std::fs::read(&real).map_err(|_| "not_found")?;
    Ok(RenderedImage {
        media_type: mt.to_string(),
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cm-img-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn decode(img: &RenderedImage) -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode(&img.data)
            .unwrap()
    }

    #[test]
    fn reads_png_and_jpg_with_media_type() {
        let d = tmp("ok");
        std::fs::write(d.join("a.png"), b"\x89PNG data").unwrap();
        std::fs::write(d.join("b.JPG"), b"jpeg").unwrap();
        let png = read_image_at(d.join("a.png").to_str().unwrap(), None, 1000).unwrap();
        assert_eq!(png.media_type, "image/png");
        assert_eq!(decode(&png), b"\x89PNG data");
        let jpg = read_image_at(d.join("b.JPG").to_str().unwrap(), None, 1000).unwrap();
        assert_eq!(jpg.media_type, "image/jpeg");
    }

    #[test]
    fn svg_maps_to_svg_xml() {
        let d = tmp("svg");
        std::fs::write(d.join("a.svg"), b"<svg/>").unwrap();
        let img = read_image_at(d.join("a.svg").to_str().unwrap(), None, 1000).unwrap();
        assert_eq!(img.media_type, "image/svg+xml");
    }

    #[test]
    fn missing_directory_and_relative_are_not_found() {
        let d = tmp("missing");
        std::fs::create_dir_all(d.join("dir.png")).unwrap();
        let err = |p: &str| read_image_at(p, None, 1000).unwrap_err();
        assert_eq!(err(d.join("gone.png").to_str().unwrap()), "not_found");
        assert_eq!(err(d.join("dir.png").to_str().unwrap()), "not_found");
        assert_eq!(err("shots/a.png"), "not_found");
    }

    #[test]
    fn non_image_extensions_are_refused() {
        let d = tmp("ext");
        std::fs::write(d.join("a.txt"), b"x").unwrap();
        std::fs::write(d.join("noext"), b"x").unwrap();
        assert_eq!(
            read_image_at(d.join("a.txt").to_str().unwrap(), None, 1000).unwrap_err(),
            "not_image"
        );
        assert_eq!(
            read_image_at(d.join("noext").to_str().unwrap(), None, 1000).unwrap_err(),
            "not_image"
        );
    }

    #[test]
    fn symlink_to_non_image_is_refused() {
        let d = tmp("link");
        std::fs::write(d.join("secret.txt"), b"x").unwrap();
        std::os::unix::fs::symlink(d.join("secret.txt"), d.join("pic.png")).unwrap();
        assert_eq!(
            read_image_at(d.join("pic.png").to_str().unwrap(), None, 1000).unwrap_err(),
            "not_image"
        );
    }

    #[test]
    fn tilde_expands_against_home() {
        let d = tmp("home");
        std::fs::write(d.join("x.png"), b"png").unwrap();
        let img = read_image_at("~/x.png", Some(&d), 1000).unwrap();
        assert_eq!(decode(&img), b"png");
        assert_eq!(
            read_image_at("~/x.png", None, 1000).unwrap_err(),
            "not_found"
        );
    }

    #[test]
    fn over_the_cap_is_too_large() {
        let d = tmp("big");
        std::fs::write(d.join("big.png"), vec![0u8; 11]).unwrap();
        assert_eq!(
            read_image_at(d.join("big.png").to_str().unwrap(), None, 10).unwrap_err(),
            "too_large"
        );
    }
}
