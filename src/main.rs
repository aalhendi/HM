use std::mem::size_of;

use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{
            LibraryLoader::GetModuleHandleA,
            Memory::{VirtualAlloc, VirtualFree, MEM_COMMIT, MEM_RELEASE, PAGE_READWRITE},
        },
        UI::WindowsAndMessaging::*,
    },
};

// TODO(aalhendi): This is a global for now.
static mut RUNNING: bool = false;
static mut BITMAP_INFO: Option<BITMAPINFO> = None;
// NOTE(aalhendi): void* to avoid specifying the type, we want windows to give us back a ptr to the bitmap memory
//  windows doesn't know (on the API lvl), what sort of flags, and therefore what kind of memory we want.
//  CreateDIBSection also can't haveThe fn can only have one signature, it cant get a u8ptr OR a u64 ptr etc. so we pass a void* and cast appropriately
//  it is used as a double ptr because we give windows an addr of a ptr which we want it to OVERWRITE into a NEW PTR which would point to where it alloc'd mem
static mut BITMAP_MEMORY: Option<*mut std::ffi::c_void> = None;
static mut BITMAP_WIDTH: i32 = 0;
static mut BITMAP_HEIGHT: i32 = 0;
const BYTES_PER_PIXEL: i32 = 4;

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
        let mut x_offset = 0;
        let mut y_offset = 0;
        let mut message = MSG::default();
        while RUNNING {
            while PeekMessageA(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if matches!(message.message, WM_QUIT) {
                    RUNNING = false;
                }

                DispatchMessageA(&message);
                let _ = TranslateMessage(&message); // TODO(aalhendi): handle zero case?
            }
            render_weird_gradient(x_offset, y_offset);

            let device_context = GetDC(window_handle);
            let mut client_rect = RECT::default();
            let _ = GetClientRect(window_handle, &mut client_rect); // TODO(aalhendi): can fail
            let window_width = client_rect.right - client_rect.left;
            let window_height = client_rect.bottom - client_rect.top;
            win32_update_window(
                device_context,
                client_rect,
                0,
                0,
                window_width,
                window_height,
            );
            ReleaseDC(window_handle, device_context);
            x_offset += 1;
            y_offset += 2;
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
                let mut client_rect = RECT::default();
                let _ = GetClientRect(window, &mut client_rect); // can fail
                let width = client_rect.right - client_rect.left;
                let height = client_rect.bottom - client_rect.top;
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
                let mut client_rect = RECT::default();
                let _ = GetClientRect(window, &mut client_rect); // TODO(aalhendi): can fail
                win32_update_window(device_context, client_rect, x, y, width, height);
                let _ = EndPaint(window, &paint);
                LRESULT(0)
            }
            _ => DefWindowProcA(window, message, wparam, lparam),
        }
    }
}

/// Resize or Initialize a Device Independent Bitmap (DIB)
unsafe fn win32_resize_dib_section(width: i32, height: i32) {
    if BITMAP_MEMORY.is_some() {
        let free_res = VirtualFree(BITMAP_MEMORY.unwrap(), 0, MEM_RELEASE);
        if let Err(e) = free_res {
            panic!("{e}");
        }
    }

    BITMAP_WIDTH = width;
    BITMAP_HEIGHT = height;

    let mut bitmap_info = BITMAPINFO::default();
    bitmap_info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bitmap_info.bmiHeader.biWidth = BITMAP_WIDTH;
    bitmap_info.bmiHeader.biHeight = -BITMAP_HEIGHT; // negative to be top-down DIB
    bitmap_info.bmiHeader.biPlanes = 1;
    bitmap_info.bmiHeader.biBitCount = 32; // 8 for red, 8 for green, 8 for blue, ask for 32 for DWORD alignment
    bitmap_info.bmiHeader.biCompression = BI_RGB.0; // Uncompressed

    BITMAP_INFO = Some(bitmap_info);

    // TODO(aalhendi): (ssylvan's suggestion) allocate this ourselves?
    // let mut bitmap_memory: *mut std::ffi::c_void = ptr::null_mut();
    let bitmap_memory_size = (BYTES_PER_PIXEL * BITMAP_WIDTH * BITMAP_HEIGHT) as usize;
    let bitmap_memory = VirtualAlloc(None, bitmap_memory_size, MEM_COMMIT, PAGE_READWRITE);

    BITMAP_MEMORY = Some(bitmap_memory);

    // TODO(aalhendi): Probably clear this to black
}

unsafe fn render_weird_gradient(blue_offset: u32, green_offset: u32) {
    let width = BITMAP_WIDTH as u32;
    let height = BITMAP_HEIGHT as u32;

    let pitch = (width * BYTES_PER_PIXEL as u32) as isize;
    let mut row = BITMAP_MEMORY.unwrap() as *const u8;
    for y in 0..height {
        let mut pixel = row as *mut u32;
        for x in 0..width {
            /*
            Padding is not put first even if its Little Endian because... Windows.
            Memory (u32): BB GG RR XX
            Register: XX RR GG BB where XX is padding 0
            */
            let blue = x + blue_offset;
            let green = y + green_offset;

            *pixel = (green << 8) | blue;
            pixel = pixel.offset(1);
        }
        row = row.offset(pitch);
    }
}

unsafe fn win32_update_window(
    device_context: HDC,
    client_rect: RECT,
    _x: i32,
    _y: i32,
    _width: i32,
    _height: i32,
) {
    let window_width = client_rect.right - client_rect.left;
    let window_height = client_rect.bottom - client_rect.top;

    StretchDIBits(
        device_context,
        // x,y,width,height,
        // x,y,width,height,
        0,
        0,
        BITMAP_WIDTH,
        BITMAP_HEIGHT,
        0,
        0,
        window_width,
        window_height,
        Some(BITMAP_MEMORY.unwrap()),
        &BITMAP_INFO.unwrap(),
        DIB_RGB_COLORS,
        SRCCOPY,
    );
}
