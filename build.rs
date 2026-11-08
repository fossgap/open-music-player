fn main() {
    println!("cargo:rerun-if-changed=resources/icons");
    println!("cargo:rerun-if-changed=resources/icons.gresource.xml");
    println!("cargo:rerun-if-changed=resources/io.github.openmusic.Player.svg");
    let output =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("icons.gresource");
    let status = std::process::Command::new("glib-compile-resources")
        .args(["--sourcedir=resources", "--target"])
        .arg(output)
        .arg("resources/icons.gresource.xml")
        .status()
        .expect("Install glib-compile-resources (libglib2.0-dev-bin on Debian)");
    assert!(status.success(), "Could not compile the application icons");
}
