//! Generates raw bindings for the selected Cronet SDK.

use std::{
    env,
    path::{Path, PathBuf},
};

fn main() {
    for key in [
        "CRONET_INCLUDE_DIR",
        "CRONET_EXPORT_INCLUDE_DIR",
        "CRONET_LIB_DIR",
        "CRONET_LIB_NAME",
        "CRONET_STATIC",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=include/cronet_rs_dev.h");
    println!("cargo:rerun-if-changed=include/cronet_export.h");
    println!("cargo:rerun-if-changed=include/cronet_rs_bidirectional.h");
    println!("cargo:rerun-if-changed=include/cronet_rs_naive.h");
    println!("cargo:rerun-if-changed=abi/cronet-go-d62042e.symbols");

    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let mut builder = bindgen::Builder::default()
        .header(manifest.join("wrapper.h").display().to_string())
        .allowlist_function("(Cronet_.*|bidirectional_stream_.*)")
        .allowlist_type("(Cronet_.*|bidirectional_stream.*|stream_engine)")
        .allowlist_var("Cronet_.*")
        .derive_default(true)
        .generate_comments(true)
        .layout_tests(false)
        .use_core()
        .clang_arg(format!("-I{}", manifest.join("include").display()));

    let using_sdk = if let Some(include_dir) = env::var_os("CRONET_INCLUDE_DIR") {
        let include_dir = PathBuf::from(include_dir);
        let header = find_header(&include_dir).unwrap_or_else(|| {
            panic!(
                "CRONET_INCLUDE_DIR={} does not contain cronet.idl_c.h",
                include_dir.display()
            )
        });
        builder = builder
            .clang_arg(format!("-I{}", include_dir.display()))
            .clang_arg(format!(
                "-DCRONET_RS_EXTERNAL_HEADER=\"{}\"",
                header.display().to_string().replace('\\', "/")
            ));
        if let Some(export_dir) = env::var_os("CRONET_EXPORT_INCLUDE_DIR") {
            builder = builder.clang_arg(format!("-I{}", PathBuf::from(export_dir).display()));
        }
        true
    } else {
        println!("cargo:warning=CRONET_INCLUDE_DIR is unset; using development declarations");
        false
    };

    if let Some(lib_dir) = env::var_os("CRONET_LIB_DIR") {
        println!(
            "cargo:rustc-link-search=native={}",
            PathBuf::from(lib_dir).display()
        );
        let kind = if env::var_os("CRONET_STATIC").is_some() {
            "static"
        } else {
            "dylib"
        };
        let name = env::var("CRONET_LIB_NAME").unwrap_or_else(|_| "cronet".into());
        println!("cargo:rustc-link-lib={kind}={name}");
    } else if using_sdk {
        println!(
            "cargo:warning=CRONET_LIB_DIR is unset; consumers must provide Cronet to the linker"
        );
    }

    let bindings = builder
        .generate()
        .expect("failed to generate Cronet bindings; install libclang and check the SDK");
    bindings
        .write_to_file(PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("bindings.rs"))
        .expect("failed to write Cronet bindings");
}

fn find_header(root: &Path) -> Option<PathBuf> {
    [
        root.join("cronet.idl_c.h"),
        root.join("cronet_c.h"),
        root.join("generated/cronet.idl_c.h"),
        root.join("include/cronet_c.h"),
        root.join("components/cronet/native/generated/cronet.idl_c.h"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}
