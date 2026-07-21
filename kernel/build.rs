fn main() {
    // Linker script do kernel, com caminho absoluto para funcionar
    // independentemente do diretorio de invocacao do cargo.
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg=-T{dir}/src/arch/aarch64/linker.ld");
    println!("cargo:rerun-if-changed=src/arch/aarch64/linker.ld");
}
