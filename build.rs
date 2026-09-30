//! Le compresseur BC7 de bevy_mod_mipmap_generator (feature `compress`,
//! crate intel_tex_2) embarque du C++ précompilé qui a besoin de la
//! bibliothèque standard C++ ; elle n'est pas liée d'office.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }
}
