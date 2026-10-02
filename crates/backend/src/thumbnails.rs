use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{RenderCache, RenderSettings, render};
use hayro::{hayro_interpret::InterpreterSettings, hayro_syntax::Pdf};
use image::{DynamicImage, ImageFormat, ImageReader};
use sha1::{Digest, Sha1};
use std::{
    fs::File,
    io::{BufWriter, Cursor},
    path::{Path, PathBuf},
    process::Command,
};
use stl_thumb::{config::Config as StlThumbnailConfig, render_to_image};

pub(crate) enum ThumbnailOutcome {
    Cached(PathBuf),
    Generated(PathBuf),
    NotImage,
}

pub(crate) fn thumbnail_for(path: &Path, directory: &Path) -> Result<ThumbnailOutcome, String> {
    if !path.is_file() {
        return Ok(ThumbnailOutcome::NotImage);
    }
    let thumbnail_path = directory.join(thumbnail_filename(path));
    if thumbnail_path.is_file() {
        return Ok(ThumbnailOutcome::Cached(thumbnail_path));
    }
    if is_three_d_model(path) {
        write_thumbnail(three_d_model_image(path)?, &thumbnail_path)?;
        return Ok(ThumbnailOutcome::Generated(thumbnail_path));
    }
    if is_video(path) {
        write_video_thumbnail(path, &thumbnail_path)?;
        return Ok(ThumbnailOutcome::Generated(thumbnail_path));
    }

    let image = match image_from_path(path).or_else(|| audio_cover_image(path)) {
        Some(image) => image,
        None if is_pdf(path) => pdf_first_page_image(path)?,
        None => return Ok(ThumbnailOutcome::NotImage),
    };
    write_thumbnail(image, &thumbnail_path)?;
    Ok(ThumbnailOutcome::Generated(thumbnail_path))
}

pub(crate) fn is_pdf(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

pub(crate) fn is_three_d_model(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["stl", "obj", "3mf"]
                .iter()
                .any(|format| extension.eq_ignore_ascii_case(format))
        })
}

pub(crate) fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            [
                "3gp", "asf", "avi", "flv", "m4v", "mkv", "mov", "mp4", "mpeg", "mpg", "ogv",
                "webm", "wmv",
            ]
            .iter()
            .any(|format| extension.eq_ignore_ascii_case(format))
        })
}

pub(crate) fn three_d_model_image(path: &Path) -> Result<DynamicImage, String> {
    let config = StlThumbnailConfig {
        model_filename: path.display().to_string(),
        width: 256,
        height: 256,
        ..Default::default()
    };

    render_to_image(&config)
        .map_err(|error| format!("could not render 3D model {}: {error}", path.display()))
}

pub(crate) fn pdf_first_page_image(path: &Path) -> Result<DynamicImage, String> {
    let data = std::fs::read(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let pdf = Pdf::new(data).map_err(|error| format!("could not parse PDF: {error:?}"))?;
    let page = pdf
        .pages()
        .iter()
        .next()
        .ok_or_else(|| "PDF has no pages".to_owned())?;
    let (width, height) = page.render_dimensions();
    let scale = 512.0 / width.max(height).max(1.0);
    let pixmap = render(
        page,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
            ..Default::default()
        },
    );
    let png = pixmap
        .into_png()
        .map_err(|error| format!("could not encode PDF preview: {error}"))?;
    ImageReader::new(Cursor::new(png))
        .with_guessed_format()
        .map_err(|error| format!("could not read PDF preview: {error}"))?
        .decode()
        .map_err(|error| format!("could not decode PDF preview: {error}"))
}

pub(crate) fn image_from_path(path: &Path) -> Option<DynamicImage> {
    let reader = ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    reader.decode().ok()
}

pub(crate) fn audio_cover_image(path: &Path) -> Option<DynamicImage> {
    use lofty::{file::TaggedFileExt, picture::PictureType};

    let audio = lofty::read_from_path(path).ok()?;
    let picture = audio
        .tags()
        .iter()
        .flat_map(|tag| tag.pictures())
        .find(|picture| picture.pic_type() == PictureType::CoverFront)
        .or_else(|| audio.tags().iter().flat_map(|tag| tag.pictures()).next())?;
    ImageReader::new(Cursor::new(picture.data()))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()
}

pub(crate) fn write_video_thumbnail(path: &Path, thumbnail_path: &Path) -> Result<(), String> {
    let directory = thumbnail_path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", thumbnail_path.display()))?;
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("could not create {}: {error}", directory.display()))?;
    let temporary_path = thumbnail_path.with_extension("tmp.png");
    let ffmpeg = std::env::var_os("IRON_FILE_FFMPEG").unwrap_or_else(|| "ffmpeg".into());
    let status = Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-ss", "00:00:00", "-i"])
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=256:256:force_original_aspect_ratio=decrease",
        ])
        .arg(&temporary_path)
        .status()
        .map_err(|error| format!("could not run ffmpeg for {}: {error}", path.display()))?;
    if !status.success() {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(format!(
            "ffmpeg could not create a thumbnail for {}",
            path.display()
        ));
    }
    std::fs::rename(&temporary_path, thumbnail_path).map_err(|error| {
        format!(
            "could not move {} to {}: {error}",
            temporary_path.display(),
            thumbnail_path.display()
        )
    })
}

pub(crate) fn write_thumbnail(image: DynamicImage, thumbnail_path: &Path) -> Result<(), String> {
    let directory = thumbnail_path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", thumbnail_path.display()))?;
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("could not create {}: {error}", directory.display()))?;
    let temporary_path = thumbnail_path.with_extension("tmp");
    let file = File::create(&temporary_path)
        .map_err(|error| format!("could not create {}: {error}", temporary_path.display()))?;
    image
        .thumbnail(256, 256)
        .write_to(&mut BufWriter::new(file), ImageFormat::Png)
        .map_err(|error| format!("could not write {}: {error}", temporary_path.display()))?;
    std::fs::rename(&temporary_path, &thumbnail_path).map_err(|error| {
        format!(
            "could not move {} to {}: {error}",
            temporary_path.display(),
            thumbnail_path.display()
        )
    })
}

pub(crate) fn thumbnail_filename(path: &Path) -> String {
    let mut hasher = Sha1::new();
    hasher.update(path.as_os_str().as_encoded_bytes());
    format!("{:x}.png", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_filename_is_the_sha1_of_the_full_path() {
        assert_eq!(
            thumbnail_filename(Path::new("/tmp/image.png")),
            "0fef0cc8ed6b0e98686a7ae869b2eda3aafce32e.png"
        );
    }

    #[test]
    fn identifies_pdf_paths_case_insensitively() {
        assert!(is_pdf(Path::new("report.pdf")));
        assert!(is_pdf(Path::new("report.PDF")));
        assert!(!is_pdf(Path::new("report.png")));
    }

    #[test]
    fn identifies_supported_three_d_model_paths_case_insensitively() {
        assert!(is_three_d_model(Path::new("model.stl")));
        assert!(is_three_d_model(Path::new("model.OBJ")));
        assert!(is_three_d_model(Path::new("model.3mf")));
        assert!(!is_three_d_model(Path::new("model.ply")));
    }

    #[test]
    fn identifies_supported_video_paths_case_insensitively() {
        assert!(is_video(Path::new("clip.mp4")));
        assert!(is_video(Path::new("recording.MKV")));
        assert!(is_video(Path::new("movie.webm")));
        assert!(!is_video(Path::new("sound.mp3")));
    }

    #[test]
    fn creates_a_thumbnail_for_a_pdf() {
        let root = std::env::temp_dir().join(format!(
            "iron-file-pdf-thumbnail-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(&root).unwrap();
        let pdf_path = root.join("document.pdf");
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0];
        for object in [
            "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
            "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> /Contents 4 0 R >>\nendobj\n",
            "4 0 obj\n<< /Length 0 >>\nstream\n\nendstream\nendobj\n",
        ] {
            offsets.push(pdf.len());
            pdf.extend_from_slice(object.as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for offset in offsets.into_iter().skip(1) {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        std::fs::write(&pdf_path, pdf).unwrap();

        let outcome = thumbnail_for(&pdf_path, &root.join("thumbnails")).unwrap();
        let ThumbnailOutcome::Generated(thumbnail_path) = outcome else {
            panic!("expected a generated PDF thumbnail");
        };
        assert!(thumbnail_path.is_file());
        assert!(image::open(&thumbnail_path).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }
}
