#[cfg(not(target_os = "linux"))]
compile_error!("linux_platform can only be built on Linux.");

pub fn run() {
    println!("Running Linux platform-specific code");
}