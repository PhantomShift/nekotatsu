
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::fs::read_dir("ui")?;
    for e in dir.into_iter().flatten() {
        let file_name_os = e.file_name();
        let file_name = file_name_os.to_str().unwrap();
        let name = file_name.trim_end_matches(".slint");
        slint_build::compile(&e.path())?;
        println!(
            "cargo:rustc-env=SLINT_INCLUDE_{}={}/{name}.rs",
            name.to_ascii_uppercase(),
            std::env::var("OUT_DIR").unwrap()
        );
        println!("cargo:warning=compiled slint file {name}, exported to SLINT_INCLUDE_{}", name.to_ascii_uppercase());
    }

    Ok(())
}