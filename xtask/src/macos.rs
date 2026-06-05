use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};

use anyhow::{Context as _, Result};
use clap::Parser;

use crate::util::{
    TempDir, absolutize, command_exists, command_stdout, ensure_command, ensure_dir, ensure_file,
    repo_root, run, with_env,
};

#[derive(Parser)]
pub(crate) struct DmgMacos {
    /// App bundle to package.
    #[arg(long, default_value = "target/release/bundle/osx/OpenLogi.app")]
    app: PathBuf,
    /// Output DMG path.
    #[arg(long, default_value = "target/release/OpenLogi.dmg")]
    output: PathBuf,
    /// Developer ID identity used to sign the DMG, and the app when packaging.
    #[arg(long, env = "OPENLOGI_SIGN_IDENTITY")]
    sign_identity: Option<String>,
    /// Branded DMG background URL.
    #[arg(
        long,
        env = "OPENLOGI_DMG_BACKGROUND_URL",
        default_value = "https://assets.openlogi.org/dmg/dmg-background.tiff"
    )]
    background_url: String,
}

pub(crate) fn package_macos(args: &DmgMacos) -> Result<()> {
    bundle_macos()?;
    if let Some(identity) = &args.sign_identity {
        sign_app(identity)?;
    } else {
        println!("==> codesign: skipped (unsigned — set OPENLOGI_SIGN_IDENTITY to sign)");
    }
    dmg_macos(args)
}

pub(crate) fn generate_macos_icns() -> Result<()> {
    let root = repo_root()?;
    let svg = root.join("design/icon/openlogi.svg");
    let output_dir = root.join("crates/openlogi-gui/icon");
    let icns_output = output_dir.join("AppIcon.icns");
    let png_output = root.join("design/icon/openlogi.png");

    ensure_file(&svg)?;
    fs::create_dir_all(&output_dir).with_context(|| {
        format!(
            "could not create icon output directory {}",
            output_dir.display()
        )
    })?;

    let work = TempDir::new("openlogi-icns")?;
    let iconset = work.path().join("AppIcon.iconset");
    fs::create_dir_all(&iconset)
        .with_context(|| format!("could not create iconset directory {}", iconset.display()))?;

    let master = work.path().join("master-1024.png");
    render_svg_master(&svg, &master)?;
    repair_icon_surface(&master)?;

    render_iconset(&iconset, |size, output| {
        run(ProcessCommand::new("sips")
            .arg("-z")
            .arg(size.to_string())
            .arg(size.to_string())
            .arg(&master)
            .arg("--out")
            .arg(output)
            .stdout(Stdio::null()))?;
        repair_icon_surface(output)
    })?;

    let logo = work.path().join("openlogi-512.png");
    run(ProcessCommand::new("sips")
        .arg("-z")
        .arg("512")
        .arg("512")
        .arg(&master)
        .arg("--out")
        .arg(&logo)
        .stdout(Stdio::null()))?;
    repair_icon_surface(&logo)?;
    fs::copy(&logo, &png_output)
        .with_context(|| format!("could not write {}", png_output.display()))?;

    run(ProcessCommand::new("iconutil")
        .arg("-c")
        .arg("icns")
        .arg(&iconset)
        .arg("-o")
        .arg(&icns_output))?;
    println!("wrote {}", icns_output.display());
    println!("wrote {}", png_output.display());
    Ok(())
}

/// Render the master SVG to a 1024×1024 PNG with a transparent background.
fn render_svg_master(svg: &Path, output: &Path) -> Result<()> {
    if let Some(rsvg) = rsvg_convert_path() {
        run(ProcessCommand::new(&rsvg)
            .arg("-w")
            .arg("1024")
            .arg("-h")
            .arg("1024")
            .arg("-b")
            .arg("transparent")
            .arg("-o")
            .arg(output)
            .arg(svg))?;
        return Ok(());
    }

    if command_exists("resvg") {
        run(ProcessCommand::new("resvg")
            .arg("--width")
            .arg("1024")
            .arg("--height")
            .arg("1024")
            .arg("--background")
            .arg("none")
            .arg(svg)
            .arg(output))?;
        return Ok(());
    }

    println!("note: no rsvg-convert/resvg — using qlmanage + icon surface repair");
    let parent = output.parent().context("master PNG has no parent directory")?;
    let _ = ProcessCommand::new("qlmanage")
        .arg("-t")
        .arg("-s")
        .arg("1024")
        .arg("-o")
        .arg(parent)
        .arg(svg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let rendered = parent.join(format!("{}.png", svg.file_name().unwrap_or_default().to_string_lossy()));
    ensure_file(&rendered)
        .with_context(|| format!("qlmanage could not render {}", svg.display()))?;
    fs::rename(&rendered, output).or_else(|_| fs::copy(&rendered, output).map(|_| ()))?;
    Ok(())
}

fn rsvg_convert_path() -> Option<PathBuf> {
    if command_exists("rsvg-convert") {
        return Some(PathBuf::from("rsvg-convert"));
    }
    for path in ["/opt/homebrew/bin/rsvg-convert", "/usr/local/bin/rsvg-convert"] {
        if Path::new(path).is_file() {
            return Some(PathBuf::from(path));
        }
    }
    None
}

/// Mouse body bounds from [`design/icon/openlogi.svg`] on a 1024×1024 canvas.
const MOUSE_X0: f32 = 356.0;
const MOUSE_X1: f32 = 668.0;
const MOUSE_Y0: f32 = 212.0;
const MOUSE_Y1: f32 = 812.0;

/// Fix exporter matting: qlmanage insets the raster on a white field and leaves
/// transparent gutters when corners are stripped. Scale content to the canvas,
/// then paint every non-mouse matte pixel with the icon's blue gradient so the
/// Dock tile is full-bleed.
fn repair_icon_surface(path: &Path) -> Result<()> {
    use image::{Rgba, RgbaImage};

    let img: RgbaImage = image::open(path)
        .with_context(|| format!("could not read PNG {}", path.display()))?
        .into_rgba8();
    let mut img = scale_content_to_fill(img)?;
    let (w, h) = img.dimensions();
    let (mx0, mx1, my0, my1) = mouse_bounds(w, h);

    for y in 0..h {
        for x in 0..w {
            if in_mouse(x, y, mx0, mx1, my0, my1) {
                continue;
            }
            let pixel = *img.get_pixel(x, y);
            if is_blue_pixel(pixel.0) || !is_matte_pixel(pixel.0) {
                continue;
            }
            img.put_pixel(x, y, Rgba(bg_gradient(y, h)));
        }
    }

    img.save(path)
        .with_context(|| format!("could not write PNG {}", path.display()))?;
    Ok(())
}

fn mouse_bounds(w: u32, h: u32) -> (u32, u32, u32, u32) {
    let mx0 = (w as f32 * MOUSE_X0 / 1024.0).round() as u32;
    let mx1 = (w as f32 * MOUSE_X1 / 1024.0).round() as u32;
    let my0 = (h as f32 * MOUSE_Y0 / 1024.0).round() as u32;
    let my1 = (h as f32 * MOUSE_Y1 / 1024.0).round() as u32;
    (mx0, mx1, my0, my1)
}

fn in_mouse(x: u32, y: u32, mx0: u32, mx1: u32, my0: u32, my1: u32) -> bool {
    x >= mx0 && x <= mx1 && y >= my0 && y <= my1
}

fn is_matte_pixel([r, g, b, a]: [u8; 4]) -> bool {
    a < 20
        || (a > 200 && r > 240 && g > 240 && b > 240)
        || (a > 200 && r < 15 && g < 15 && b < 15)
}

fn is_blue_pixel([r, g, b, a]: [u8; 4]) -> bool {
    a > 160 && b > 160 && r < 160 && g < 210
}

fn is_content_pixel(
    pixel: image::Rgba<u8>,
    x: u32,
    y: u32,
    mx0: u32,
    mx1: u32,
    my0: u32,
    my1: u32,
) -> bool {
    in_mouse(x, y, mx0, mx1, my0, my1) || !is_matte_pixel(pixel.0)
}

/// Scale up when an exporter (qlmanage) inset the artwork on a white square.
fn scale_content_to_fill(img: image::RgbaImage) -> Result<image::RgbaImage> {
    use image::imageops::{self, FilterType};

    let (w, h) = img.dimensions();
    let (mx0, mx1, my0, my1) = mouse_bounds(w, h);

    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;
    for y in 0..h {
        for x in 0..w {
            if !is_content_pixel(*img.get_pixel(x, y), x, y, mx0, mx1, my0, my1) {
                continue;
            }
            found = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    if !found {
        return Ok(img);
    }

    let cw = max_x - min_x + 1;
    let ch = max_y - min_y + 1;
    if cw.min(ch) as f32 >= w as f32 * 0.96 {
        return Ok(img);
    }

    let cropped = imageops::crop_imm(&img, min_x, min_y, cw, ch).to_image();
    Ok(imageops::resize(&cropped, w, h, FilterType::Lanczos3))
}

/// Vertical gradient matching `openlogi.svg` `#bg` (#5C9DFF → #1D4ED8).
fn bg_gradient(y: u32, h: u32) -> [u8; 4] {
    let t = y as f32 / h.saturating_sub(1).max(1) as f32;
    let r = (92.0 + (29.0 - 92.0) * t).round() as u8;
    let g = (157.0 + (78.0 - 157.0) * t).round() as u8;
    let b = (255.0 + (216.0 - 255.0) * t).round() as u8;
    [r, g, b, 255]
}

fn render_iconset<F>(iconset: &Path, mut render: F) -> Result<()>
where
    F: FnMut(u16, &Path) -> Result<()>,
{
    for size in [16, 32, 128, 256, 512] {
        render(size, &iconset.join(format!("icon_{size}x{size}.png")))?;
        render(
            size * 2,
            &iconset.join(format!("icon_{size}x{size}@2x.png")),
        )?;
    }
    Ok(())
}

pub(crate) fn bundle_macos() -> Result<()> {
    let root = repo_root()?;
    let xcode_env = xcode_env()?;

    println!("==> app icon");
    generate_macos_icns()?;

    if env::var("OPENLOGI_BUNDLE_ASSETS").as_deref() == Ok("1") {
        println!("==> device assets: bundling (offline build)");
        run(with_env(
            ProcessCommand::new("cargo")
                .arg("run")
                .arg("-p")
                .arg("openlogi")
                .arg("--release")
                .arg("--")
                .arg("assets")
                .arg("sync")
                .current_dir(&root),
            &xcode_env,
        ))?;
    } else {
        println!("==> device assets: on-demand (not bundled; fetched at first launch)");
        let assets = root.join("crates/openlogi-gui/assets");
        if assets.exists() {
            fs::remove_dir_all(&assets)
                .with_context(|| format!("could not remove {}", assets.display()))?;
        }
        fs::create_dir_all(&assets)
            .with_context(|| format!("could not create {}", assets.display()))?;
    }

    println!("==> bundle (.app)");
    if !command_exists("cargo-bundle") {
        let mut install = ProcessCommand::new("cargo");
        install
            .arg("install")
            .arg("cargo-bundle")
            .arg("--locked")
            .env("CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER", "/usr/bin/cc");
        run(with_env(&mut install, &xcode_env))?;
    }
    run(with_env(
        ProcessCommand::new("cargo")
            .arg("bundle")
            .arg("--release")
            .current_dir(root.join("crates/openlogi-gui")),
        &xcode_env,
    ))?;

    let app = root.join("target/release/bundle/osx/OpenLogi.app");
    ensure_dir(&app)?;
    println!();
    println!("Bundle ready: {}", app.display());
    Ok(())
}

fn xcode_env() -> Result<Vec<(String, String)>> {
    let developer_dir = env::var("OPENLOGI_DEVELOPER_DIR")
        .unwrap_or_else(|_| "/Applications/Xcode.app/Contents/Developer".to_string());
    let sdkroot = command_stdout(
        ProcessCommand::new("/usr/bin/xcrun")
            .arg("--sdk")
            .arg("macosx")
            .arg("--show-sdk-path")
            .env("DEVELOPER_DIR", &developer_dir),
    )?;
    Ok(vec![
        ("DEVELOPER_DIR".to_string(), developer_dir),
        ("SDKROOT".to_string(), sdkroot.trim().to_string()),
    ])
}

pub(crate) fn dmg_macos(args: &DmgMacos) -> Result<()> {
    let root = repo_root()?;
    let app = absolutize(&root, &args.app);
    let output = absolutize(&root, &args.output);
    ensure_dir(&app)?;
    ensure_command("create-dmg")?;

    println!("==> dmg background");
    let background = root.join("target/release/dmg-background.tiff");
    if let Some(parent) = background.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    run(ProcessCommand::new("curl")
        .arg("-fsSL")
        .arg(&args.background_url)
        .arg("-o")
        .arg(&background))
    .with_context(|| {
        format!(
            "failed to fetch DMG background from {}",
            args.background_url
        )
    })?;

    println!("==> dmg");
    if output.exists() {
        fs::remove_file(&output)
            .with_context(|| format!("could not remove {}", output.display()))?;
    }

    // Geometry is locked to the painted 760×480 background. `create-dmg` uses
    // outer window dimensions, so add the 32pt Finder title bar and keep icon
    // coordinates relative to the 760×480 content area.
    run(ProcessCommand::new("create-dmg")
        .arg("--volname")
        .arg("OpenLogi")
        .arg("--background")
        .arg(&background)
        .arg("--window-pos")
        .arg("240")
        .arg("120")
        .arg("--window-size")
        .arg("760")
        .arg("512")
        .arg("--icon-size")
        .arg("128")
        .arg("--icon")
        .arg("OpenLogi.app")
        .arg("212")
        .arg("250")
        .arg("--app-drop-link")
        .arg("548")
        .arg("250")
        .arg("--hide-extension")
        .arg("OpenLogi.app")
        .arg(&output)
        .arg(&app))?;

    if let Some(identity) = &args.sign_identity {
        sign_dmg(identity, &output)?;
    }

    println!();
    println!("done → {}", output.display());
    Ok(())
}

fn sign_app(identity: &str) -> Result<()> {
    let app = repo_root()?.join("target/release/bundle/osx/OpenLogi.app");
    println!("==> codesign ({identity})");
    run(ProcessCommand::new("codesign")
        .arg("--force")
        .arg("--deep")
        .arg("--options")
        .arg("runtime")
        .arg("--timestamp")
        .arg("--sign")
        .arg(identity)
        .arg(&app))?;
    run(ProcessCommand::new("codesign")
        .arg("--verify")
        .arg("--deep")
        .arg("--strict")
        .arg(&app))
}

fn sign_dmg(identity: &str, dmg: &Path) -> Result<()> {
    println!("==> codesign dmg ({identity})");
    run(ProcessCommand::new("codesign")
        .arg("--force")
        .arg("--timestamp")
        .arg("--sign")
        .arg(identity)
        .arg(dmg))?;
    run(ProcessCommand::new("codesign")
        .arg("--verify")
        .arg("--verbose=2")
        .arg(dmg))
}
