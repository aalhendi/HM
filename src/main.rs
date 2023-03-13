use windows::{
    core::*,
    Win32::Foundation::*,
    Win32::System::LibraryLoader::GetModuleHandleA,
    Win32::{
        Graphics::Gdi::{
            BeginPaint, EndPaint, PatBlt, BLACKNESS, PAINTSTRUCT, ROP_CODE, WHITENESS,
        },
        UI::WindowsAndMessaging::*,
    },
};

fn main() -> Result<()> {
    unsafe {
        let instance = GetModuleHandleA(None)?;
        debug_assert!(instance.0 != 0);

        let wc = WNDCLASSA {
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hInstance: instance,
            lpszClassName: s!("HandmadeHeroWindowClass"),

            style: CS_HREDRAW | CS_VREDRAW | CS_OWNDC,
            lpfnWndProc: Some(wndproc),
            ..Default::default()
        };

        let atom = RegisterClassA(&wc);
        debug_assert!(atom != 0);

        let window_handle = CreateWindowExA(
            WINDOW_EX_STYLE::default(), // 0
            wc.lpszClassName,
            s!("Handmade Hero"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            instance,
            None,
        );

        debug_assert!(window_handle != HWND(0));

        let mut message = MSG::default();
        // Bool casting, WM_QUIT is false
        while GetMessageA(&mut message, None, 0, 0).into() {
            DispatchMessageA(&message);
        }

        Ok(())
    }
}

extern "system" fn wndproc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match message {
            WM_SIZE => {
                println!("WM_SIZE");
                LRESULT(0)
            }
            WM_CLOSE => {
                println!("WM_CLOSE");
                LRESULT(0)
            }
            WM_DESTROY => {
                println!("WM_DESTROY");
                LRESULT(0)
            }
            WM_ACTIVATEAPP => {
                println!("WM_ACTIVATE");
                LRESULT(0)
            }
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let hdc = BeginPaint(window, &mut paint);
                let width = paint.rcPaint.right - paint.rcPaint.left;
                let height = paint.rcPaint.bottom - paint.rcPaint.top;
                let x = paint.rcPaint.left;
                let y = paint.rcPaint.top;
                static mut ROP: ROP_CODE = WHITENESS;

                PatBlt(hdc, x, y, width, height, ROP);
                if ROP == WHITENESS {
                    ROP = BLACKNESS;
                } else {
                    ROP = WHITENESS;
                }
                EndPaint(window, &mut paint);
                LRESULT(0)
            }
            _ => DefWindowProcA(window, message, wparam, lparam),
        }
    }
}
