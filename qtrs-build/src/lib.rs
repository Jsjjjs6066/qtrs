//! Build helpers for qtrs: compile a project's Qt Designer `.ui` and
//! Rcc resource `.qrc` files into the final binary.
//!
//! # Why
//!
//! qtrs loads `.ui` files at *runtime* through `QUiLoader`, and loads
//! images / icons / fonts from the *filesystem* through `QPixmap` etc.
//! When you ship a single binary you usually don't want to carry those
//! loose asset files around. This crate runs Qt's own `uic` and `rcc`
//! tools at *compile time*, turns the generated C++ code into a static
//! library, and links it into your binary with `whole-archive` semantics
//! so that Qt's automatic resource registration kicks in the moment the
//! program starts.
//!
//! # Quick start
//!
//! ```toml
//! [build-dependencies]
//! qtrs-build = "0.1"
//! ```
//!
//! ```no_run
//! // build.rs
//! fn main() {
//!     qtrs_build::Ui::embed().expect("embed ui files");
//!     qtrs_build::Rcc::embed().expect("embed resource files");
//! }
//! ```
//!
//! After that:
//!
//! - `<your-package>/ui/*.ui` is exposed by Qt under `:/qrc/<name>.ui`
//!   (pass that path to `UiLoader::load`).
//! - `<your-package>/resources/*.qrc` resources are available, unchanged,
//!   under `:/...`.
//!
//! # How it works
//!
//! 1. Every `*.ui` file found under `<package>/ui` is re-listed into a
//!    synthetic `.qrc` that maps each `<name>.ui` to a resource at
//!    `:/qrc/<name>.ui`, and validated with `uic` (so malformed forms fail
//!    the build instead of bombing at runtime).
//! 2. Every `*.qrc` file found under `<package>/resources` (recursively),
//!    plus the synthetic `.ui` qrc, is compiled with `rcc` into the exact C
//!    resource-object code Qt generates, including its `initializer` static
//!    constructor.
//! 3. The C++ is built into a static library and linked with the
//!    `+whole-archive` modifier so no object file is dropped — this is
//!    what makes the static (destructor-less) initializers actually run,
//!    registering the resources in Qt's in-memory resource tree before
//!    `main()` executes.
//!
//! # Notes
//!
//! - When no match is found, the emitted library is empty (it keeps a
//!   single marker symbol). The final binary simply has no embedded
//!   resources — no error is raised.
//! - Linking requires a C++ linker for the `cc` build (same as qtrs
//!   itself). You normally call these helpers from a normal `build.rs`
//!   of a *binary* crate.
#![allow(clippy::needless_doctest_main)] // examples are build.rs snippets

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Embedded virtual file prefix used for generated `.ui` sources.
pub const UI_PREFIX: &str = "qrc";

/// The directory scanned for `.ui` files (`<package root>/ui`).
pub const UI_DIR: &str = "ui";
/// The directory scanned for `.qrc` files (`<package root>/resources`).
pub const QRC_DIR: &str = "resources";

/// Name candidates for the `rcc` binary, most preferred first.
fn rcc_candidates() -> Vec<String> {
    vec![
        "rcc6".into(),
        "rcc-qt6".into(),
        "rcc5".into(),
        "rcc-qt5".into(),
        "rcc".into(),
    ]
}

/// Name candidates for the `uic` binary, most preferred first.
fn uic_candidates() -> Vec<String> {
    vec![
        "uic6".into(),
        "uic-qt6".into(),
        "uic5".into(),
        "uic-qt5".into(),
        "uic".into(),
    ]
}

/// qmake/rcc/uic lookup configuration.
///
/// `add_qt_bin(&mut self, dir)` appends a directory to the search path so
/// you can point it at, e.g. Qt6's `QT_HOST_BINS`.
#[derive(Debug, Clone, Default)]
pub struct QtTools {
    bins: Vec<PathBuf>,
    noop: bool,
}

impl QtTools {
    /// Build from PATH plus the common Qt5/Qt6 bin directories.
    pub fn unix() -> Self {
        Self::default()
            .add_qt_bin(PathBuf::from("/usr/lib/qt6/bin"))
            .add_qt_bin(PathBuf::from("/usr/lib/qt5/bin"))
    }

    /// Same as [`Self::unix`]; kept as an explicit alias.
    pub fn unix5() -> Self {
        Self::unix()
    }

    /// Same as [`Self::unix`] but with Qt6's bin dir first.
    pub fn unix6() -> Self {
        Self::default()
            .add_qt_bin(PathBuf::from("/usr/lib/qt6/bin"))
    }

    /// Add a directory to the search path for `uic`/`rcc`. The directory
    /// need not exist yet; it is only consulted lazily.
    pub fn add_qt_bin(mut self, dir: impl Into<PathBuf>) -> Self {
        self.bins.push(dir.into());
        self
    }

    /// Force a no-op (e.g. in CI where Qt tools are absent).
    pub fn force_noop(mut self) -> Self {
        self.noop = true;
        self
    }
}

/// Namespace for `.ui` file embedding.
///
/// ```no_run
/// qtrs_build::Ui::embed().expect("embed ui files");
/// // or with explicit tools:
/// qtrs_build::Ui::embed_with(qtrs_build::QtTools::unix6());
/// ```
pub struct Ui;

/// Namespace for `.qrc` file embedding.
///
/// ```no_run
/// qtrs_build::Rcc::embed().expect("embed resource files");
/// ```
pub struct Rcc;

impl Ui {
    /// Compile every `.ui` file under `<package>/ui` into the binary using
    /// tools discovered from `PATH`.
    pub fn embed() -> Result<(), String> {
        Ui::embed_with(QtTools::unix())
    }

    /// Like [`Self::embed`] but with an explicit Qt tool configuration.
    pub fn embed_with(tools: QtTools) -> Result<(), String> {
        compile_ui(&tools)
    }
}

impl Rcc {
    /// Compile every `.qrc` file under `<package>/resources` into the
    /// binary using tools discovered from `PATH`.
    pub fn embed() -> Result<(), String> {
        Rcc::embed_with(QtTools::unix())
    }

    /// Like [`Self::embed`] but with an explicit Qt tool configuration.
    pub fn embed_with(tools: QtTools) -> Result<(), String> {
        compile_rcc(&tools)
    }
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()))
}

/// Collect `*.<ext>` files under `root` (recursively), sorted by path.
fn collect(root: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if root.exists() {
        collect_dir(root, ext, &mut out);
        out.sort();
    }
    out
}

fn collect_dir(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_dir(&path, ext, out);
        } else if path.extension().and_then(OsStr::to_str) == Some(ext) {
            out.push(path);
        }
    }
}

/// Locate the first runnable binary matching a candidate name, walking PATH
/// then the configured bin dirs.
fn which(tools: &QtTools, candidates: &[String]) -> Option<PathBuf> {
    if tools.noop {
        return None;
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            for name in candidates {
                let p = dir.join(name);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    for dir in &tools.bins {
        for name in candidates {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Build a generated `.cpp` file (and any extra files) into a `lib*.a`,
/// returning its path. Qt include paths are added so the generated code can
/// `#include <QFile>` etc. You normally compile with a Qt toolchain; the
/// helper asks `qmake` for the header locations.
fn build_static_lib(
    out_root: &Path,
    files: &[&Path],
    lib_name: &str,
) -> Result<PathBuf, String> {
    let out_dir = out_root.join("lib");
    fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;

    let mut build = cc::Build::new();
    build.cpp(true).cargo_metadata(false);
    build.flag_if_supported("-Wno-unused-variable");
    build.flag_if_supported("-Wno-unused-function");
    for f in files {
        build.file(f);
    }

    // Add Qt include dirs (mirrors qtrs's own build.rs discovery).
    for include in qt_include_dirs() {
        build.include(&include);
    }

    build.out_dir(&out_dir);
    build.compile(lib_name);

    Ok(out_dir.join(format!("lib{lib_name}.a")))
}

/// Resolve Qt's include directories via `qmake -query QT_INSTALL_HEADERS`,
/// plus the module subdirs (QtCore, QtCore/6.x/compat, qt6/QtCore, etc.).
fn qt_include_dirs() -> Vec<PathBuf> {
    let base = qmake_headers().unwrap_or_default();
    let modules = ["QtCore", "QtGui", "QtWidgets"];

    let roots = vec![base.clone(), base.join("qt6"), base.join("Qt6")];
    let mut all = vec![base];
    for root in roots {
        if root.exists() && !all.contains(&root) {
            all.push(root.clone());
        }
        for module in modules {
            let p = root.join(module);
            if p.exists() && !all.contains(&p) {
                all.push(p);
            }
        }
    }
    all
}

/// Run `qmake -query QT_INSTALL_HEADERS`.
fn qmake_headers() -> Option<PathBuf> {
    for qmake in &["qmake6", "qmake", "qmake-qt5", "qmake-qt6"] {
        let out = Command::new(qmake)
            .args(["-query", "QT_INSTALL_HEADERS"])
            .output()
            .ok()?;
        if !out.status.success() {
            continue;
        }
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !path.is_empty() {
            let p = PathBuf::from(&path);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

/// Emit the `rustc-link-lib` directives for a whole-archive static lib.
///
/// `lib_name` is e.g. `qtrs_ui`: `-lqtrs_ui` will resolve the archive
/// `libqtrs_ui.a` produced by [`build_static_lib`].
fn link_whole_archive(archive: &Path, lib_name: &str) {
    let parent = archive.parent().expect("archive has parent");
    println!("cargo:rustc-link-search=native={}", parent.display());
    println!("cargo:rustc-link-lib=static:+whole-archive={}", lib_name);
    println!("cargo:rerun-if-changed={}", archive.display());
}

/// A filesystem-safe identifier derived from a file name.
fn sanitize_ident(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() || out.chars().next().unwrap().is_ascii_digit() {
        out.insert(0, '_');
    }
    out
}

fn compile_ui(tools: &QtTools) -> Result<(), String> {
    let root = manifest_dir();
    let ui_dir = root.join(UI_DIR);
    let files = collect(&ui_dir, "ui");

    // Emit a (possibly empty) archive no matter what, so the final binary
    // links deterministically whether or not there are any .ui files.
    if files.is_empty() {
        let out = empty_lib(&root, "ui", "qtrs_ui")?;
        link_whole_archive(&out, "qtrs_ui");
        return Ok(());
    }

    let Some((uic, rcc)) = embed_tools(tools) else {
        // Tools missing: fall back to an empty archive (silent degradation).
        let out = empty_lib(&root, "ui", "qtrs_ui")?;
        link_whole_archive(&out, "qtrs_ui");
        return Ok(());
    };

    let out = compile_ui_core(&root, &ui_dir, &files, &uic, &rcc)?;
    link_whole_archive(&out, "qtrs_ui");
    Ok(())
}

/// Locate both `uic` and `rcc`, or `None` if either is unavailable.
fn embed_tools(tools: &QtTools) -> Option<(PathBuf, PathBuf)> {
    let uic = which(tools, &uic_candidates())?;
    let rcc = which(tools, &rcc_candidates())?;
    Some((uic, rcc))
}

fn compile_ui_core(
    root: &Path,
    ui_dir: &Path,
    files: &[PathBuf],
    uic: &Path,
    rcc: &Path,
) -> Result<PathBuf, String> {
    let out_dir = root.join("target").join("qtrs-build").join("ui");
    let gen_dir = out_dir.join("generated");
    fs::create_dir_all(&gen_dir).map_err(|e| e.to_string())?;

    // Build a synthetic .qrc that maps every <name>.ui to a resource at
    // :/qrc/<name>.ui, then compile it with rcc exactly like any user .qrc.
    // This makes the .ui XML a real, registered Qt resource so that
    // UiLoader::load(":/qrc/<name>.ui") resolves through the resource
    // file-system at runtime. `uic` is run on each file purely to validate
    // it (the generated header is not linked).
    let mut qrc = String::from("<RCC>\n    <qresource prefix=\"/\">\n");
    for file in files {
        let rel = file.strip_prefix(ui_dir).map_err(|e| e.to_string())?;
        // Relative path with the .ui extension stripped, kept as a flat-ish
        // identifier (subdir separators become `_`) so nested .ui files get
        // unique symbols and unique `:/qrc/...` paths.
        let rel_stem = rel
            .with_extension("")
            .to_string_lossy()
            .into_owned();
        let ident = sanitize_ident(
            &rel_stem.replace('\\', "/").replace('/', "_"),
        );
        let alias = format!("{}/{}.ui", UI_PREFIX, ident);

        // Validate through uic.
        let hdr = gen_dir.join(format!("ui_{}.h", ident));
        let status = Command::new(uic)
            .args(["-o", &hdr.to_string_lossy()])
            .arg(file)
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!(
                "qtrs-build: `{}` failed for {}",
                uic.display(),
                file.display()
            ));
        }
        println!("cargo:rerun-if-changed={}", file.display());

        // The alias must be relative to the synthetic .qrc's directory, so
        // emit an absolute file path and let rcc resolve it.
        qrc.push_str(&format!(
            "        <file alias=\"{}\">{}</file>\n",
            alias,
            file.display()
        ));
    }
    qrc.push_str("    </qresource>\n</RCC>\n");

    let qrc_path = gen_dir.join("ui.qrc");
    fs::write(&qrc_path, qrc).map_err(|e| e.to_string())?;
    println!("cargo:rerun-if-changed={}", qrc_path.display());

    // Compile the synthetic .qrc the same way as a user .qrc. Pass a single
    // "file" (the generated qrc) so relative alias resolution is stable.
    let qrc_refs: Vec<&Path> = vec![qrc_path.as_path()];
    compile_qrc_into_archive(&out_dir, rcc, &qrc_refs, &gen_dir, "qtrs_ui", "qtrs_ui")
}

fn compile_rcc(tools: &QtTools) -> Result<(), String> {
    let root = manifest_dir();
    let res_dir = root.join(QRC_DIR);
    let files = collect(&res_dir, "qrc");

    if files.is_empty() {
        let out = empty_lib(&root, "qrc", "qtrs_qrc")?;
        link_whole_archive(&out, "qtrs_qrc");
        return Ok(());
    }

    let Some(rcc) = which(tools, &rcc_candidates()) else {
        let out = empty_lib(&root, "qrc", "qtrs_qrc")?;
        link_whole_archive(&out, "qtrs_qrc");
        return Ok(());
    };

    let out = compile_rcc_core(&root, &res_dir, &files, &rcc)?;
    link_whole_archive(&out, "qtrs_qrc");
    Ok(())
}

fn compile_rcc_core(
    root: &Path,
    res_dir: &Path,
    files: &[PathBuf],
    rcc: &Path,
) -> Result<PathBuf, String> {
    let out_dir = root.join("target").join("qtrs-build").join("qrc");
    let gen_dir = out_dir.join("generated");
    fs::create_dir_all(&gen_dir).map_err(|e| e.to_string())?;

    // User .qrc files resolve relative <file> entries against the location
    // of each .qrc (i.e. the resources dir), so run rcc with that as CWD.
    let qrc_refs: Vec<&Path> = files.iter().map(|p| p.as_path()).collect();
    compile_qrc_into_archive(&out_dir, rcc, &qrc_refs, res_dir, "qtrs_qrc", "qtrs_qrc")
}

/// Run `rcc` over a list of `.qrc` files (from `rcc_cwd`), compile the
/// generated C++ into a `whole-archive`-linkable static library, and return
/// the archive path. Every generated TU carries its own `initializer` that
/// registers the resources at startup; whole-archive linking ensures the
/// initializer is actually pulled into the final binary.
///
/// `name_prefix` seeds the `-name` rcc option so symbols never collide
/// between the `.ui` and `.qrc` archives.
fn compile_qrc_into_archive(
    out_dir: &Path,
    rcc: &Path,
    qrc_files: &[&Path],
    rcc_cwd: &Path,
    lib_name: &str,
    name_prefix: &str,
) -> Result<PathBuf, String> {
    let gen_dir = out_dir.join("generated");
    fs::create_dir_all(&gen_dir).map_err(|e| e.to_string())?;

    let mut cpp_paths: Vec<PathBuf> = Vec::new();
    for (i, file) in qrc_files.iter().enumerate() {
        let res_name = format!("{}_{}", name_prefix, i);
        let cpp = gen_dir.join(format!("{}.cpp", res_name));
        let status = Command::new(rcc)
            .arg("-name")
            .arg(&res_name)
            .arg("-o")
            .arg(&cpp)
            .current_dir(rcc_cwd)
            .arg(file.file_name().unwrap())
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!(
                "qtrs-build: `{}` failed for {}",
                rcc.display(),
                file.display()
            ));
        }
        cpp_paths.push(cpp);
        println!("cargo:rerun-if-changed={}", file.display());
    }

    // Compile the generated TUs; whole-archive pulls each initializer in.
    let mut build = cc::Build::new();
    build.cpp(true).cargo_metadata(false);
    build.flag_if_supported("-Wno-unused-variable");
    build.flag_if_supported("-Wno-unused-function");
    for p in &cpp_paths {
        build.file(p);
    }
    let lib_dir = out_dir.join("lib");
    fs::create_dir_all(&lib_dir).map_err(|e| e.to_string())?;
    build.out_dir(&lib_dir);
    build.compile(lib_name);

    Ok(lib_dir.join(format!("lib{lib_name}.a")))
}

/// Build an empty static lib so `+whole-archive` always has a member.
fn empty_lib(root: &Path, tag: &str, lib_name: &str) -> Result<PathBuf, String> {
    let out_dir = root.join("target").join("qtrs-build").join(tag);
    let gen_dir = out_dir.join("generated");
    fs::create_dir_all(&gen_dir).map_err(|e| e.to_string())?;
    let cpp = gen_dir.join("empty.cpp");
    fs::write(&cpp, "// qtrs-build — empty placeholder.\nint qtrs_empty_marker_;\n")
        .map_err(|e| e.to_string())?;
    build_static_lib(&out_dir, &[&cpp], lib_name)
}
