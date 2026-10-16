use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    built::write_built_file().expect("Failed to acquire build-time information");

    embed_stdlib();
}

/// Embed the standard library modules (`stdlib/lib`, minus test modules) into the
/// compiler, as a list of (module path, source) pairs.
fn embed_stdlib() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let lib = manifest_dir
        .join("..")
        .join("..")
        .join("stdlib")
        .join("lib");

    // Cargo scans the whole directory, so adding or removing modules triggers a rebuild too.
    println!("cargo:rerun-if-changed={}", lib.display());

    let mut modules = Vec::new();
    collect_modules(&lib, &lib, &mut modules);
    modules.sort();

    let mut out = String::from("pub const MODULES: &[(&str, &str)] = &[\n");
    for (name, path) in modules {
        out.push_str(&format!("    ({name:?}, include_str!({path:?})),\n"));
    }
    out.push_str("];\n");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(out_dir.join("stdlib.rs"), out).expect("Failed to write the standard library");
}

fn collect_modules(root: &Path, dir: &Path, modules: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).expect("Failed to read the standard library sources") {
        let path = entry.unwrap().path();

        if path.is_dir() {
            collect_modules(root, &path, modules);
        } else if let Some(file_name) = path.file_name().and_then(|s| s.to_str())
            && file_name.ends_with(".ak")
            && !file_name.ends_with(".test.ak")
        {
            let name = path
                .strip_prefix(root)
                .unwrap()
                .components()
                .map(|c| c.as_os_str().to_str().unwrap())
                .collect::<Vec<_>>()
                .join("/");

            modules.push((name, path.to_str().unwrap().to_string()));
        }
    }
}
