//! Who the program is: its name, version, app id, folders, design file extension and
//! environment prefix. SolidCraft is the default. A product built on SolidCraft (its own `main`
//! calls `solidcraft_app::run` or `solidcraft_cli::run`) sets its brand first thing, so that it
//! keeps its settings, autosaves, crash reports, control tokens and parameter packs apart from
//! an installed SolidCraft's, saves designs under its own extension (and still opens
//! `.solidcraft` files), reads its own environment variables and says its own name in the
//! window, the app bar, the start page, About and the messages. It never reaches SolidCraft's
//! update feed, and SolidCraft's issue tracker and documentation only when it names them.

use std::path::PathBuf;
use std::sync::OnceLock;

/// Draws a square app icon `size` pixels wide: RGBA, row by row.
pub type IconFn = fn(usize) -> Result<Vec<u8>, String>;

/// A program's identity. Every text is `'static`: a brand is a constant of the program.
#[derive(Clone, Copy, Debug)]
pub struct Brand {
    /// The name people read: window title, app bar, start page, About, messages.
    pub name: &'static str,
    /// The product's version (SolidCraft's is the workspace version).
    pub version: &'static str,
    /// Reverse-DNS app id: the window's app id (Wayland, desktop entry).
    pub app_id: &'static str,
    /// The per-user folder for settings and data, `/` between levels (`Tonghao/Rebar3D`): as
    /// given on Windows and macOS, in lower case on other systems.
    pub folder: &'static str,
    /// The extension designs are saved with, without the dot.
    pub design_ext: &'static str,
    /// The environment variables' prefix (`SOLIDCRAFT_`).
    pub env_prefix: &'static str,
    /// The desktop program's file name, without `.exe`.
    pub app: &'static str,
    /// The command line's file name, without `.exe`.
    pub cli: &'static str,
    /// The documentation; `None` hides the Documentation links.
    pub docs_url: Option<&'static str>,
    /// Where reports are filed (a GitHub new-issue page); `None`: reports are copied instead.
    pub issues_url: Option<&'static str>,
    /// The release notes; `None` hides What's New.
    pub releases_url: Option<&'static str>,
    /// The app icon (window, taskbar, `cli icon`). `None`: SolidCraft's own mark for SolidCraft,
    /// no icon for a product built on it.
    pub icon: Option<IconFn>,
}

/// SolidCraft itself.
pub const SOLIDCRAFT: Brand = Brand {
    name: "SolidCraft",
    version: env!("CARGO_PKG_VERSION"),
    app_id: "ai.storyteller.solidcraft",
    folder: "SolidCraft",
    design_ext: "solidcraft",
    env_prefix: crate::env::PREFIX,
    app: "solidcraft",
    cli: "solidcraft-cli",
    docs_url: Some("https://docs.solidcraft.io/"),
    issues_url: Some("https://github.com/bherbruck/solidcraft/issues/new"),
    releases_url: Some("https://github.com/bherbruck/solidcraft/releases/latest"),
    icon: None,
};

impl Brand {
    /// Is this SolidCraft itself, not a product built on it?
    pub fn is_solidcraft(&self) -> bool {
        self.app_id == SOLIDCRAFT.app_id
    }

    /// The per-user folder as a relative path: as spelled, or in lower case (`lower`, for the
    /// systems that name app folders so).
    pub fn folder_path(&self, lower: bool) -> PathBuf {
        self.folder.split('/').map(|s| if lower { s.to_lowercase() } else { s.to_string() }).collect()
    }
}

static BRAND: OnceLock<Brand> = OnceLock::new();

/// Make `brand` this program's: first thing in `main`, before anything reads it. Refused when
/// it is not a product's brand ([`check`]) or once the brand has been read or set.
pub fn set(brand: Brand) -> Result<(), String> {
    check(&brand)?;
    BRAND.set(brand).map_err(|_| format!("the program is already {}: set its brand first thing in main", get().name))
}

/// This program's brand: the one [`set`] made, else SolidCraft.
pub fn get() -> &'static Brand {
    BRAND.get_or_init(|| SOLIDCRAFT)
}

/// Is this program SolidCraft itself?
pub fn is_default() -> bool {
    get().is_solidcraft()
}

/// Why `b` can't be a product's brand, if it can't: every text well formed, the URLs `https`,
/// and no app id, folder, extension, prefix or program name of SolidCraft's own.
pub fn check(b: &Brand) -> Result<(), String> {
    let word = |s: &str, extra: &[char]| !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || extra.contains(&c));
    let text = |s: &str| !s.trim().is_empty() && s.len() <= 64 && !s.chars().any(char::is_control);
    if !text(b.name) || !text(b.version) || b.version.contains(char::is_whitespace) {
        return Err(format!("brand name `{}` or version `{}`: 1 to 64 bytes, no control characters", b.name, b.version));
    }
    let id = b.app_id;
    if !word(id, &['.', '-', '_']) || !id.contains('.') || id.starts_with('.') || id.ends_with('.') || id.contains("..") {
        return Err(format!("brand app id `{id}`: reverse DNS, such as com.example.product"));
    }
    let segments: Vec<&str> = b.folder.split('/').collect();
    if segments.len() > 4 || !segments.iter().all(|s| word(s, &['.', '-', '_']) && !s.starts_with('.')) {
        return Err(format!("brand folder `{}`: up to 4 names of letters, digits, `.`, `-`, `_`, joined by `/`", b.folder));
    }
    let ext = b.design_ext;
    if !word(ext, &[]) || ext.contains(|c: char| c.is_ascii_uppercase()) || ["json", "solvecraft"].contains(&ext) {
        return Err(format!("brand design extension `{ext}`: lower-case letters and digits, without the dot"));
    }
    let prefix = b.env_prefix;
    let upper = !prefix.contains(|c: char| c.is_ascii_lowercase()) && !prefix.starts_with(|c: char| c.is_ascii_digit());
    if prefix.len() < 2 || !prefix.ends_with('_') || !word(prefix, &['_']) || !upper {
        return Err(format!("brand environment prefix `{prefix}`: upper case, ending in `_`"));
    }
    if !word(b.app, &['-', '_']) || !word(b.cli, &['-', '_']) {
        return Err(format!("brand program names `{}`, `{}`: letters, digits, `-`, `_`", b.app, b.cli));
    }
    for url in [b.docs_url, b.issues_url, b.releases_url].into_iter().flatten() {
        if !url.starts_with("https://") || url.len() > 512 || url.contains(|c: char| c.is_whitespace() || c.is_control()) {
            return Err(format!("brand URL `{url}`: https, no spaces"));
        }
    }
    let s = &SOLIDCRAFT;
    let own = [
        ("app id", b.app_id, s.app_id),
        ("folder", b.folder, s.folder),
        ("design extension", b.design_ext, s.design_ext),
        ("environment prefix", b.env_prefix, s.env_prefix),
        ("desktop program", b.app, s.app),
        ("command line", b.cli, s.cli),
    ];
    if let Some((what, ..)) = own.iter().find(|(_, ours, theirs)| ours.eq_ignore_ascii_case(theirs)) {
        return Err(format!("brand {what} is SolidCraft's own: a product built on SolidCraft needs its own"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const PRODUCT: Brand = Brand {
        name: "Tonghao Rebar3D",
        version: "1.2.3",
        app_id: "ai.tonghao.rebar3d",
        folder: "Tonghao/Rebar3D",
        design_ext: "rebar3d",
        env_prefix: "REBAR3D_",
        app: "rebar3d",
        cli: "rebar3d-cli",
        docs_url: None,
        issues_url: None,
        releases_url: None,
        icon: None,
    };

    #[test]
    fn a_products_brand_is_checked() {
        assert_eq!(check(&PRODUCT), Ok(()));
        for bad in [
            Brand { name: " ", ..PRODUCT },
            Brand { version: "1 2", ..PRODUCT },
            Brand { app_id: "rebar3d", ..PRODUCT },
            Brand { app_id: "ai..rebar3d", ..PRODUCT },
            Brand { folder: "../Rebar3D", ..PRODUCT },
            Brand { folder: "Tonghao//Rebar3D", ..PRODUCT },
            Brand { folder: "Tong hao", ..PRODUCT },
            Brand { folder: r"Tonghao\Rebar3D", ..PRODUCT },
            Brand { design_ext: ".rebar3d", ..PRODUCT },
            Brand { design_ext: "Rebar3D", ..PRODUCT },
            Brand { design_ext: "json", ..PRODUCT },
            Brand { env_prefix: "REBAR3D", ..PRODUCT },
            Brand { env_prefix: "rebar3d_", ..PRODUCT },
            Brand { cli: "rebar3d cli", ..PRODUCT },
            Brand { docs_url: Some("http://example.com/"), ..PRODUCT },
            // SolidCraft's own.
            Brand { app_id: "ai.storyteller.solidcraft", ..PRODUCT },
            Brand { folder: "solidcraft", ..PRODUCT },
            Brand { design_ext: "solidcraft", ..PRODUCT },
            Brand { env_prefix: "SOLIDCRAFT_", ..PRODUCT },
            Brand { cli: "solidcraft-cli", ..PRODUCT },
        ] {
            assert!(check(&bad).is_err(), "{bad:?}");
        }
        assert!(check(&SOLIDCRAFT).is_err(), "SolidCraft's brand is no product's");
    }

    #[test]
    fn the_folder_is_a_relative_path() {
        assert_eq!(PRODUCT.folder_path(false), Path::new("Tonghao").join("Rebar3D"));
        assert_eq!(PRODUCT.folder_path(true), Path::new("tonghao").join("rebar3d"));
        assert_eq!(SOLIDCRAFT.folder_path(false), Path::new("SolidCraft"));
        assert_eq!(SOLIDCRAFT.folder_path(true), Path::new("solidcraft"));
    }

    #[test]
    fn tests_run_as_solidcraft() {
        assert!(is_default() && get().name == "SolidCraft" && !PRODUCT.is_solidcraft());
    }
}
