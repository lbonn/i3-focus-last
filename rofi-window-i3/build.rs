use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/wrapper.h");

    let glib = pkg_config::Config::new()
        .atleast_version("2.72")
        .probe("glib-2.0")
        .unwrap();
    let cairo = pkg_config::Config::new()
        .atleast_version("1.16")
        .probe("cairo")
        .unwrap();

    let bindings = bindgen::Builder::default()
        .clang_args(
            glib.include_paths
                .iter()
                .map(|path| format!("-I{}", path.to_string_lossy())),
        )
        .clang_args(
            cairo
                .include_paths
                .iter()
                .map(|path| format!("-I{}", path.to_string_lossy())),
        )
        .header("src/wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
