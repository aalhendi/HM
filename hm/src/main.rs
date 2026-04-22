mod platform {
    #[cfg(target_os = "windows")]
    pub fn run() {
        win32_platform::run();
    }

    #[cfg(target_os = "linux")]
    pub fn run() {
        linux_platform::run();
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    pub fn run() {
        eprintln!("Unsupported platform. HM currently routes only Windows and Linux.");
        std::process::exit(1);
    }
}

fn main() {
    platform::run();
}
