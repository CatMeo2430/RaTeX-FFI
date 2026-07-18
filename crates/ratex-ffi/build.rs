fn main() {
  #[cfg(target_os = "windows")]
  {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let def = std::path::Path::new(&manifest_dir).join("ratex_ffi.def");
    println!("cargo:rerun-if-changed={}", def.display());
    // Only apply .def to the DLL (cdylib); this crate also builds staticlib for Android.
    println!("cargo:rustc-cdylib-link-arg=/DEF:{}", def.display());
  }
}
