//! Embeds the browser workbench into the program.
//!
//! The folder is `$BLACKCAD_WEB_DIR`, or `dist/web/` (made by `cargo xtask
//! web`). Without it the program still builds, for development, and serves
//! from a folder given with `--web-dir`; `BLACKCAD_REQUIRE_WEB=1` (the
//! release scripts set it) makes a missing folder an error instead.
//!
//! On Windows it also compiles `windows/blackcad.rc` (icon, version
//! information) with rc.exe when found; `BLACKCAD_REQUIRE_WINRES=1` makes a
//! missing rc.exe an error.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=BLACKCAD_WEB_DIR");
    println!("cargo:rerun-if-env-changed=BLACKCAD_REQUIRE_WEB");
    println!("cargo:rerun-if-env-changed=BLACKCAD_REQUIRE_WINRES");
    println!("cargo:rerun-if-changed=build.rs");
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap_or_default());
    embed_web(&manifest, &out);
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        resources(&manifest, &out);
    }
}

fn embed_web(manifest: &Path, out: &Path) {
    let dir = match env::var_os("BLACKCAD_WEB_DIR") {
        Some(d) => PathBuf::from(d),
        None => manifest.join("..").join("..").join("dist").join("web"),
    };
    println!("cargo:rerun-if-changed={}", dir.display());
    let require = env::var_os("BLACKCAD_REQUIRE_WEB").is_some_and(|v| v == "1");
    let mut files = Vec::new();
    if dir.join("index.html").is_file() && dir.join("core.wasm").is_file() {
        collect(&dir, &dir, &mut files);
        files.sort();
    } else if require {
        panic!(
            "{} has no index.html and core.wasm: run `cargo xtask web` first (or set BLACKCAD_WEB_DIR)",
            dir.display()
        );
    } else {
        println!(
            "cargo:warning=no built web app in {}: this build serves only from --web-dir",
            dir.display()
        );
    }
    let mut src = String::from("pub static ASSETS: &[(&str, &[u8])] = &[\n");
    for (rel, path) in &files {
        let abs = fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        let _ = writeln!(src, "    ({rel:?}, include_bytes!({:?})),", abs.to_string_lossy());
    }
    src.push_str("];\n");
    if let Err(e) = fs::write(out.join("assets.rs"), src) {
        panic!("cannot write assets.rs: {e}");
    }
}

fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(root, &p, files);
        } else if let Ok(rel) = p.strip_prefix(root) {
            let rel: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            files.push((rel.join("/"), p.clone()));
        }
    }
}

fn resources(manifest: &Path, out: &Path) {
    let rc_file = manifest.join("windows").join("blackcad.rc");
    println!("cargo:rerun-if-changed={}", rc_file.display());
    let require = env::var_os("BLACKCAD_REQUIRE_WINRES").is_some_and(|v| v == "1");
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let nums: Vec<String> = version
        .split(['.', '-'])
        .take(3)
        .map(|s| s.chars().take_while(char::is_ascii_digit).collect::<String>())
        .map(|s| if s.is_empty() { "0".into() } else { s })
        .collect();
    let (a, b, c) = (
        nums.first().cloned().unwrap_or_default(),
        nums.get(1).cloned().unwrap_or_default(),
        nums.get(2).cloned().unwrap_or_default(),
    );
    let Some(rc) = find_rc() else {
        let msg = "rc.exe not found: the program has no icon or version information";
        if require {
            panic!("{msg}");
        }
        println!("cargo:warning={msg}");
        return;
    };
    let res = out.join("blackcad.res");
    let status = Command::new(&rc)
        .arg("/nologo")
        .arg(format!("/dBC_VER_COMMA={a},{b},{c},0"))
        .arg(format!("/dBC_VER_TEXT=\\\"{version}\\\""))
        .arg(format!("/fo{}", res.display()))
        .arg(&rc_file)
        .current_dir(rc_file.parent().unwrap_or(manifest))
        .status();
    match status {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", res.display()),
        other => {
            let msg = format!("rc.exe failed ({other:?})");
            if require {
                panic!("{msg}");
            }
            println!("cargo:warning={msg}");
        }
    }
}

fn find_rc() -> Option<PathBuf> {
    if let Some(p) = env::var_os("PATH") {
        for d in env::split_paths(&p) {
            let c = d.join("rc.exe");
            if c.is_file() {
                return Some(c);
            }
        }
    }
    let kits = PathBuf::from(env::var_os("ProgramFiles(x86)")?).join("Windows Kits").join("10").join("bin");
    let mut vers: Vec<PathBuf> = fs::read_dir(kits).ok()?.flatten().map(|e| e.path()).collect();
    vers.sort();
    vers.iter().rev().map(|v| v.join("x64").join("rc.exe")).find(|p| p.is_file())
}
