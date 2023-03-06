use windows::{core::*, Win32::Foundation::*, Win32::UI::WindowsAndMessaging::*};

fn main() -> Result<()> {
    unsafe {
        MessageBoxA(
            HWND(0),
            s!("Hello, World."),
            s!("lolping"),
            MB_OK | MB_ICONINFORMATION,
        );
    }

    Ok(())
}
