fn main() {
    // Force WINDOWS subsystem on Windows to prevent console window from appearing
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-arg-bins=/SUBSYSTEM:WINDOWS");
        println!("cargo:rustc-link-arg-bins=/ENTRY:mainCRTStartup");
    }

    tauri_build::build()
}
