use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn sources(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("engine source directory") {
        let path = entry.expect("source entry").path();
        if path.is_dir() {
            sources(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let mut files = vec![root.join("Cargo.toml"), root.join("build.rs")];
    sources(&root.join("src"), &mut files);
    files.sort();
    println!("cargo:rerun-if-changed=src");
    let mut hash = Sha256::new();
    for path in files {
        let relative = path
            .strip_prefix(&root)
            .unwrap()
            .to_str()
            .unwrap()
            .replace('\\', "/");
        println!("cargo:rerun-if-changed={relative}");
        let bytes = fs::read(&path).expect("engine source");
        hash.update((relative.len() as u64).to_le_bytes());
        hash.update(relative.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    println!(
        "cargo:rustc-env=NOLAN_ENGINE_REVISION={:x}",
        hash.finalize()
    );
}
