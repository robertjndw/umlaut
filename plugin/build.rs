//! Compiles Graphviz's dot layout (plus a small shim, see graphviz/shim.c) into the
//! plugin. The same C sources build for the host (native `cargo test`) and for
//! wasm32-unknown-unknown, where they are compiled with wasi-sdk's clang against
//! wasi-libc; `make wasm` then strips the WASI imports with `wasi-stub`.
//!
//! The approach (Graphviz as a Typst wasm plugin, built from the unmodified release
//! tarball) follows the diagraph package, https://github.com/Robotechnic/diagraph.
//!
//! Graphviz is downloaded once into `.deps/` at the repository root, checked against a
//! pinned SHA-256 and reused afterwards. Set `UMLAUT_GRAPHVIZ_SRC` to use an unpacked
//! source tree you already have (for offline builds).

use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const VERSION: &str = "14.1.4";
const SHA256: &str = "f81f7c5a9d262e2b8346d91b2c007fc927d224cc5f55ceb8eba847dfc4c6ec20";

/// Directories under lib/ that make up the dot layout, with the files each one needs.
/// Everything the layout does not reach is dropped by the linker.
const SOURCES: &[(&str, &[&str])] = &[
    ("cdt", &["dtclose", "dtdisc", "dtextract", "dtflatten", "dthash", "dtmethod", "dtopen", "dtrenew", "dtrestore", "dtsize", "dtstat", "dtstrhash", "dttree", "dtview", "dtwalk"]),
    (
        "cgraph",
        &[
            "agerror", "apply", "attr", "edge", "graph", "grammar", "id", "imap", "io", "node", "node_induce", "obj", "rec", "refstr", "scan", "subg", "utils", "write",
        ],
    ),
    (
        "common",
        &[
            "arrows", "colxlate", "ellipse", "textspan", "textspan_lut", "args", "globals", "htmllex", "htmlparse", "htmltable", "input", "pointset", "postproc", "routespl", "splines",
            "psusershape", "timing", "labels", "ns", "shapes", "utils", "geom", "taper", "output", "emit",
        ],
    ),
    ("dotgen", &["acyclic", "class1", "class2", "cluster", "compound", "conc", "decomp", "fastgr", "flat", "dotinit", "mincross", "position", "rank", "sameport", "dotsplines", "aspect"]),
    (
        "gvc",
        &["gvrender", "gvlayout", "gvdevice", "gvloadimage", "gvcontext", "gvjobs", "gvevent", "gvplugin", "gvconfig", "gvtextlayout", "gvusershape", "gvc"],
    ),
    ("label", &["xlabels", "index", "node", "rectangle", "split.q"]),
    ("ortho", &["fPQ", "maze", "ortho", "partition", "rawgraph", "sgraph", "trapezoid"]),
    ("pack", &["ccomps", "pack"]),
    ("pathplan", &["cvt", "inpoly", "route", "shortest", "shortestpth", "solvers", "triang", "util", "visibility"]),
    ("util", &["arena", "base64", "gv_find_me", "gv_fopen", "list", "random", "xml"]),
    ("xdot", &["xdot"]),
];

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let repo = manifest.parent().unwrap().to_path_buf();
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=graphviz");
    println!("cargo:rerun-if-env-changed=UMLAUT_GRAPHVIZ_SRC");
    println!("cargo:rerun-if-env-changed=WASI_SDK_PATH");

    let src = match env::var_os("UMLAUT_GRAPHVIZ_SRC") {
        Some(p) => PathBuf::from(p),
        None => fetch(&repo.join(".deps")),
    };

    let mut build = cc::Build::new();
    build
        .include(manifest.join("graphviz"))
        .include(&src)
        .include(src.join("lib"))
        .include(src.join("lib/cgraph"))
        .include(src.join("lib/cdt"))
        .include(src.join("lib/pathplan"))
        .include(src.join("lib/gvc"))
        .include(src.join("lib/common"))
        .include(src.join("lib/label"))
        .include(src.join("lib/ortho"))
        .include(src.join("lib/pack"))
        .include(src.join("lib/xdot"))
        .include(src.join("lib/util"))
        // The tarball is third-party code: its warnings are not ours to fix.
        .warnings(false)
        .extra_warnings(false)
        .flag_if_supported("-Wno-everything")
        .define("NDEBUG", None)
        .opt_level_str("3");
    for (dir, files) in SOURCES {
        for f in *files {
            build.file(src.join("lib").join(dir).join(format!("{f}.c")));
        }
    }
    build.file(src.join("plugin/dot_layout/gvplugin_dot_layout.c"));
    build.file(src.join("plugin/dot_layout/gvlayout_dot_layout.c"));
    build.file(manifest.join("graphviz/shim.c"));

    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        wasm_toolchain(&mut build, &repo);
    }
    build.compile("graphviz");
}

/// Points cc at wasi-sdk (installed by `make tools`): the system clang cannot target
/// wasm, and there is no libc for wasm32-unknown-unknown to link against otherwise.
fn wasm_toolchain(build: &mut cc::Build, repo: &Path) {
    let sdk = env::var_os("WASI_SDK_PATH").map(PathBuf::from).unwrap_or_else(|| repo.join(".tools/wasi-sdk"));
    let clang = sdk.join("bin/clang");
    if !clang.exists() {
        panic!("wasi-sdk not found at {}: run `make tools` (or set WASI_SDK_PATH)", sdk.display());
    }
    let sysroot = sdk.join("share/wasi-sysroot");
    build
        .compiler(clang)
        .archiver(sdk.join("bin/llvm-ar"))
        .flag("--target=wasm32-wasip1")
        .flag(format!("--sysroot={}", sysroot.display()))
        // Typst's wasm runtime has no threads or exceptions; keep to the baseline the
        // Rust side is built with.
        .flag("-mno-exception-handling")
        .flag("-fno-exceptions")
        // Graphviz includes <signal.h> and times things with clock(); wasi-libc offers
        // inert stand-ins for both.
        .define("_WASI_EMULATED_SIGNAL", None)
        .define("_WASI_EMULATED_PROCESS_CLOCKS", None);
    // The Rust side links against wasi-libc; libc.a lives in the per-target subdirectory.
    println!("cargo:rustc-link-search=native={}", sysroot.join("lib/wasm32-wasip1").display());
    println!("cargo:rustc-link-lib=static=c");
    println!("cargo:rustc-link-lib=static=wasi-emulated-signal");
    println!("cargo:rustc-link-lib=static=wasi-emulated-process-clocks");
    // Compiler builtins for 128-bit arithmetic and similar, shipped next to clang.
    if let Some(dir) = builtins_dir(&sdk) {
        println!("cargo:rustc-link-search=native={}", dir.display());
        println!("cargo:rustc-link-lib=static=clang_rt.builtins");
    }
}

fn builtins_dir(sdk: &Path) -> Option<PathBuf> {
    let root = sdk.join("lib/clang");
    for version in fs::read_dir(root).ok()?.flatten() {
        for sub in ["lib/wasm32-unknown-wasip1", "lib/wasip1", "lib/wasm32-wasip1"] {
            let dir = version.path().join(sub);
            if dir.join("libclang_rt.builtins.a").exists() || dir.join("libclang_rt.builtins-wasm32.a").exists() {
                return Some(dir);
            }
        }
    }
    None
}

/// Downloads and unpacks the Graphviz release into `deps` unless it is already there.
fn fetch(deps: &Path) -> PathBuf {
    let dir = deps.join(format!("graphviz-{VERSION}"));
    if dir.join("lib/gvc/gvc.c").exists() {
        return dir;
    }
    fs::create_dir_all(deps).unwrap();
    let archive = deps.join(format!("graphviz-{VERSION}.tar.gz"));
    if !archive.exists() || sha256(&archive) != SHA256 {
        let url = format!("https://gitlab.com/api/v4/projects/4207231/packages/generic/graphviz-releases/{VERSION}/graphviz-{VERSION}.tar.gz");
        run(Command::new("curl").args(["-fsSL", "--retry", "3", "-o"]).arg(&archive).arg(&url), "download Graphviz (is curl installed?)");
        let got = sha256(&archive);
        if got != SHA256 {
            let _ = fs::remove_file(&archive);
            panic!("Graphviz archive checksum mismatch: expected {SHA256}, got {got}");
        }
    }
    // Unpack beside the target and rename, so an interrupted build never leaves a
    // half-extracted tree that looks complete.
    let tmp = deps.join(format!("graphviz-{VERSION}.partial"));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    // Only what the build and the license notice need; the rest of the tarball is docs,
    // tests and other layout engines.
    let top = format!("graphviz-{VERSION}");
    let wanted = ["lib", "plugin/dot_layout", "graphviz_version.h", "builddate.h", "COPYING", "cpl1.0.txt"];
    run(
        Command::new("tar").arg("-xzf").arg(&archive).arg("-C").arg(&tmp).arg("--strip-components=1").args(wanted.iter().map(|w| format!("{top}/{w}"))),
        "unpack Graphviz (is tar installed?)",
    );
    fs::rename(&tmp, &dir).unwrap();
    dir
}

fn sha256(path: &Path) -> String {
    let bytes = fs::read(path).unwrap();
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn run(cmd: &mut Command, what: &str) {
    let status = cmd.status().unwrap_or_else(|e| panic!("could not {what}: {e}"));
    if !status.success() {
        panic!("could not {what}: {status}");
    }
}
