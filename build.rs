//! Le compresseur BC7 (crate intel_tex_2, voir src/texture_bake.rs)
//! embarque du C++ précompilé qui a besoin de la bibliothèque standard
//! C++ ; elle n'est pas liée d'office.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }
}
