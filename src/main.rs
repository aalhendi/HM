use std::{mem::size_of, ptr};

use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::{
            BeginPaint, CreateCompatibleDC, CreateDIBSection, DeleteObject, EndPaint,
            StretchDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
            PAINTSTRUCT, SRCCOPY,
        },
        System::LibraryLoader::GetModuleHandleA,
        UI::WindowsAndMessaging::*,
    },
};

// TODO(aalhendi): This is a global for now.
static mut RUNNING: bool = false;
static mut BITMAP_INFO: Option<BITMAPINFO> = None;
static mut BITMAP_MEMORY: Option<*mut std::ffi::c_void> = None;
static mut BITMAPHANDLE: Option<HBITMAP> = None;
static mut BITMAPDEVICECONTEXT: Option<HDC> = None;

fn main() -> Result<()> {
    unsafe {
        let module_handle = GetModuleHandleA(None)?;

        let wc = WNDCLASSA {
            // TODO(aalhendi): Check if HREDRAW/VREDRAW/OWNDC still matter
            style: CS_HREDRAW | CS_VREDRAW | CS_OWNDC,
            lpfnWndProc: Some(win32_main_window_callback),
            hInstance: GetModuleHandleA(None)?.into(),
            lpszClassName: s!("HandmadeHeroWindowClass"),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };

        let atom = RegisterClassA(&wc);
        if atom == 0 {
            // TODO(aalhendi): Logging
            // return
        }
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
            module_handle,
            None,
        );

        if window_handle == HWND(0) {
            // TODO(aalhendi): Logging
            // return
        }
        debug_assert!(window_handle != HWND(0));

        RUNNING = true;
        let mut message = MSG::default();
        // Bool casting, WM_QUIT is false

        while RUNNING {
            if GetMessageA(&mut message, None, 0, 0).0 > 0 {
                DispatchMessageA(&message);
                let _ = TranslateMessage(&message); // TODO(aalhendi): handle zero case?
            } else {
                break;
            }
        }

        Ok(())
    }
}

extern "system" fn win32_main_window_callback(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        // Update the buffer in size, blip the buffer on paint
        match message {
            WM_SIZE => {
                let mut rect = RECT::default();
                let _ = GetClientRect(window, &mut rect); // can fail
                let width = rect.right - rect.left;
                let height = rect.bottom - rect.top;
                win32_resize_dib_section(width, height);
                println!("WM_SIZE");
                LRESULT(0)
            }
            WM_CLOSE => {
                // TODO(aalhendi): Handle this with a message to the user?
                RUNNING = false;
                LRESULT(0)
            }
            WM_DESTROY => {
                // TODO(aalhendi): Handle this as an error - recreate window?
                RUNNING = false;
                LRESULT(0)
            }
            WM_ACTIVATEAPP => {
                println!("WM_ACTIVATE");
                LRESULT(0)
            }
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let device_context = BeginPaint(window, &mut paint);
                let width = paint.rcPaint.right - paint.rcPaint.left;
                let height = paint.rcPaint.bottom - paint.rcPaint.top;
                let x = paint.rcPaint.left;
                let y = paint.rcPaint.top;
                win32_update_window(device_context, x, y, width, height);
                let _ = EndPaint(window, &paint);
                LRESULT(0)
            }
            _ => DefWindowProcA(window, message, wparam, lparam),
        }
    }
}

/// Resize or Initialize a Device Independent Bitmap (DIB)
unsafe fn win32_resize_dib_section(width: i32, height: i32) {
    if BITMAPHANDLE.is_some_and(|h| !(DeleteObject(h).as_bool())) {
        panic!("Unable to delete bitmap handle");
    }

    // TODO(aalhendi): Should we recreate these under certain circumstances?
    if BITMAPDEVICECONTEXT.is_none() {
        BITMAPDEVICECONTEXT = Some(CreateCompatibleDC(HDC::default()));
    }

    let mut bitmap_info = BITMAPINFO::default();
    bitmap_info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bitmap_info.bmiHeader.biWidth = width;
    bitmap_info.bmiHeader.biHeight = height;
    bitmap_info.bmiHeader.biPlanes = 1;
    bitmap_info.bmiHeader.biBitCount = 32; // 8 for red, 8 for green, 8 for blue, ask for 32 for DWORD alignment
    bitmap_info.bmiHeader.biCompression = BI_RGB.0; // Uncompressed

    BITMAP_INFO = Some(bitmap_info);

    // TODO(aalhendi): (ssylvan's suggestion) allocate this ourselves?
    let mut bitmap_memory: *mut std::ffi::c_void = ptr::null_mut();

    let bitmap_handle = CreateDIBSection(
        BITMAPDEVICECONTEXT.unwrap(),
        &BITMAP_INFO.unwrap(),
        DIB_RGB_COLORS,
        &mut bitmap_memory,
        HANDLE(0),
        0u32,
    );

    if bitmap_handle.is_err() {
        panic!("Could not create BMP handle");
    }

    BITMAPHANDLE = Some(bitmap_handle.unwrap());
    BITMAP_MEMORY = Some(bitmap_memory);
}

unsafe fn win32_update_window(device_context: HDC, x: i32, y: i32, width: i32, height: i32) {
    StretchDIBits(
        device_context,
        x,
        y,
        width,
        height,
        x,
        y,
        width,
        height,
        Some(BITMAP_MEMORY.unwrap()),
        &BITMAP_INFO.unwrap(),
        DIB_RGB_COLORS,
        SRCCOPY,
    );
}
