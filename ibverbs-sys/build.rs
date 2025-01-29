use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("failed to get current directory");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    println!("cargo:include={manifest_dir}/vendor/rdma-core/build/include");
    println!("cargo:rustc-link-search=native={manifest_dir}/vendor/rdma-core/build/lib");
    println!("cargo:rustc-link-lib=ibverbs");

    if Path::new("vendor/rdma-core/CMakeLists.txt").exists() {
        // don't touch source dir if not necessary
    } else if Path::new(".git").is_dir() {
        // initialize and update submodules
        Command::new("git")
            .args(["submodule", "update", "--init"])
            .status()
            .expect("Failed to update submodules.");
    } else {
        assert!(
            Path::new("vendor/rdma-core").is_dir(),
            "vendor source not included"
        );
    }

    // build vendor/rdma-core
    // note that we only build it to generate the bindings!
    eprintln!("run cmake");
    let built_in = cmake::Config::new("vendor/rdma-core")
        .define("NO_MAN_PAGES", "1")
        // cmake crate defaults CMAKE_INSTALL_PREFIX to the output directory
        //
        //   https://github.com/rust-lang/cmake-rs/blob/94da9de2ea79ab6cad572e908864a160cf4847a9/src/lib.rs#L699-L703
        //
        // this results in overly long runtime paths on docs.rs, which then fail the build. it also
        // causes sadness for users trying to build since the bindings may fail to build for the
        // same reason (see https://github.com/jonhoo/rust-ibverbs/pull/41 for what was an
        // incomplete fix).
        //
        // since we never actually _install_ anything when building here, we should be able to
        // safely set this to any short path. simply by convention we set it to `/usr`.
        .define("CMAKE_INSTALL_PREFIX", "/usr")
        .no_build_target(true)
        .build();
    let built_in = built_in
        .to_str()
        .expect("build directory path is not valid UTF-8");

    let verb_h = "vendor/rdma-core/libibverbs/verbs.h";

    // generate the bindings
    eprintln!("run bindgen");
    let bindings = bindgen::Builder::default()
        .header(verb_h)
        .clang_arg(format!("-I{built_in}/include/"))
        .allowlist_function("ibv_.*")
        .allowlist_type("ibv_.*")
        .allowlist_var("IBV_LINK_LAYER_.*")
        .bitfield_enum("ibv_access_flags")
        .bitfield_enum("ibv_qp_attr_mask")
        .bitfield_enum("ibv_wc_flags")
        .bitfield_enum("ibv_send_flags")
        .bitfield_enum("ibv_port_cap_flags")
        .constified_enum_module("ibv_qp_type")
        .constified_enum_module("ibv_qp_state")
        .constified_enum_module("ibv_port_state")
        .constified_enum_module("ibv_wc_opcode")
        .constified_enum_module("ibv_wr_opcode")
        .constified_enum_module("ibv_wc_status")
        //.constified_enum_module("IBV_WC_.*")
        //.constified_enum_module("IBV_WR_.*")
        //.constified_enum_module("IBV_QPS_.*")
        //.constified_enum_module("IBV_PORT_.*")
        .derive_default(true)
        .derive_debug(true)
        .prepend_enum_name(false)
        .blocklist_type("ibv_wc")
        .wrap_static_fns(true)
        .size_t_is_usize(true)
        .generate()
        .expect("Unable to generate bindings");

    let output_path = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // This is the path to the object file.
    let obj_path = output_path.join("extern.o");
    // This is the path to the static library file.
    let lib_path = output_path.join("libextern.a");

    // Compile the generated wrappers into an object file.
    let mut clang = std::process::Command::new("clang");
    clang
        .arg("-O")
        .arg("-c")
        .arg("-o")
        .arg(&obj_path)
        .arg(std::env::temp_dir().join("bindgen").join("extern.c"))
        .arg("-include")
        .arg(verb_h)
        .arg("-I.");

    let clang_output = clang.output().unwrap();

    eprintln!("clang {:?}", clang.get_args().collect::<Vec<_>>());

    if !clang_output.status.success() {
        panic!(
            "Could not compile object fileXXX:\n{}",
            String::from_utf8_lossy(&clang_output.stderr)
        );
    }

    let lib_output = Command::new("ar")
        .arg("rcs")
        .arg(lib_path)
        .arg(obj_path)
        .output()
        .unwrap();
    if !lib_output.status.success() {
        panic!(
            "Could not emit library file:\n{}",
            String::from_utf8_lossy(&lib_output.stderr)
        );
    }

    println!(
        "cargo:rustc-link-search=native={}",
        output_path.to_string_lossy()
    );

    // Tell cargo to statically link against the `libextern` static library.
    println!("cargo:rustc-link-lib=static=extern");

    // write the bindings to the $OUT_DIR/bindings.rs file.
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Could not write bindings");
}
