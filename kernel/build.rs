fn main() {
    // Linker script da arquitetura alvo, com caminho absoluto para funcionar
    // independentemente do diretorio de invocacao do cargo.
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    println!("cargo:rustc-link-arg=-T{dir}/src/arch/{arch}/linker.ld");
    println!("cargo:rerun-if-changed=src/arch/{arch}/linker.ld");
}
