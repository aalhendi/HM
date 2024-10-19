use std::{mem::size_of, ptr};

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

const BYTES_PER_PIXEL: i32 = 4;
// TODO(aalhendi): This is a global for now.
static mut GLOBAL_RUNNING: bool = false;
static mut GLOBAL_BACKBUFFER: Win32OffscreenBuffer = Win32OffscreenBuffer {
    info: unsafe { core::mem::zeroed() }, // alloc'ed in resize, called v.early in main fn
    memory: ptr::null_mut(),
    width: 0,
    height: 0,
    pitch: 0,
};

struct Win32WindowDimension {
    width: i32,
    height: i32,
}

impl From<HWND> for Win32WindowDimension {
    fn from(window_handle: HWND) -> Self {
        let mut client_rect = RECT::default();

        unsafe {
            let _ = GetClientRect(window_handle, &mut client_rect); // TODO(aalhendi): can fail
        }

        Self {
            width: client_rect.right - client_rect.left,
            height: client_rect.bottom - client_rect.top,
        }
    }
}
struct Win32OffscreenBuffer {
    // NOTE(aalhendi): pixels are always 32-bits wide, Memory Order BB GG RR XX
    info: BITMAPINFO,
    // NOTE(aalhendi): void* to avoid specifying the type, we want windows to give us back a ptr to the bitmap memory
    //  windows doesn't know (on the API lvl), what sort of flags, and therefore what kind of memory we want.
    //  CreateDIBSection also can't haveThe fn can only have one signature, it cant get a u8ptr OR a u64 ptr etc. so we pass a void* and cast appropriately
    //  it is used as a double ptr because we give windows an addr of a ptr which we want it to OVERWRITE into a NEW PTR which would point to where it alloc'd mem
    memory: *mut std::ffi::c_void,
    width: i32,
    height: i32,
    pitch: isize,
}

impl Win32OffscreenBuffer {
    unsafe fn render_weird_gradient(&self, blue_offset: u32, green_offset: u32) {
        let width = self.width as u32;
        let height = self.height as u32;

        let mut row = self.memory as *const u8;
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
            row = row.offset(self.pitch);
        }
    }

    unsafe fn win32_copy_buffer_to_window(&self, device_context: HDC, width: i32, height: i32) {
        // TODO(aalhendi): aspect ratio correction
        // TODO(aalhendi): play with stretch modes
        StretchDIBits(
            device_context,
            0,
            0,
            width,
            height,
            0,
            0,
            self.width,
            self.height,
            Some(self.memory),
            &self.info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }

    /// Resize or Initialize a Device Independent Bitmap (DIB)
    unsafe fn win32_resize_dib_section(&mut self, width: i32, height: i32) {
        if !self.memory.is_null() {
            let free_res = VirtualFree(self.memory, 0, MEM_RELEASE);
            if let Err(e) = free_res {
                panic!("{e}");
            }
        }

        self.width = width;
        self.height = height;

        self.info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        self.info.bmiHeader.biWidth = self.width;
        // NOTE(aalhendi): When bHeight is negative, it clues Windows to treat the bitmap as top-down rather than bottom-up. This means tht the first three bytes are for the top-left pixel.
        self.info.bmiHeader.biHeight = -self.height;
        self.info.bmiHeader.biPlanes = 1;
        self.info.bmiHeader.biBitCount = 32; // 8 for red, 8 for green, 8 for blue, ask for 32 for DWORD alignment
        self.info.bmiHeader.biCompression = BI_RGB.0; // Uncompressed

        let bitmap_memory_size = (BYTES_PER_PIXEL * self.width * self.height) as usize;
        self.memory = VirtualAlloc(None, bitmap_memory_size, MEM_COMMIT, PAGE_READWRITE);

        self.pitch = (width * BYTES_PER_PIXEL) as isize;
        // TODO(aalhendi): Probably clear this to black
    }
}

fn main() -> Result<()> {
    unsafe {
        let module_handle = GetModuleHandleA(None)?;

        GLOBAL_BACKBUFFER.win32_resize_dib_section(1280, 720);

        let wc = WNDCLASSA {
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
        )
        .unwrap(); // TODO(aalhendi): use expect(), remove redundant debug assert.

        if window_handle.is_invalid() {
            // TODO(aalhendi): Logging
            // return
        }
        debug_assert!(!window_handle.is_invalid());

        GLOBAL_RUNNING = true;
        let mut x_offset = 0;
        let mut y_offset = 0;
        while GLOBAL_RUNNING {
            let mut message = MSG::default();
            while PeekMessageA(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if matches!(message.message, WM_QUIT) {
                    GLOBAL_RUNNING = false;
                }

                DispatchMessageA(&message);
                let _ = TranslateMessage(&message); // TODO(aalhendi): handle zero case?
            }
            GLOBAL_BACKBUFFER.render_weird_gradient(x_offset, y_offset);

            let device_context = GetDC(window_handle);
            let dims = Win32WindowDimension::from(window_handle);
            GLOBAL_BACKBUFFER.win32_copy_buffer_to_window(device_context, dims.width, dims.height);
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
            WM_CLOSE => {
                // TODO(aalhendi): Handle this with a message to the user?
                GLOBAL_RUNNING = false;
                LRESULT(0)
            }
            WM_DESTROY => {
                // TODO(aalhendi): Handle this as an error - recreate window?
                GLOBAL_RUNNING = false;
                LRESULT(0)
            }
            WM_ACTIVATEAPP => {
                println!("WM_ACTIVATE");
                LRESULT(0)
            }
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let device_context = BeginPaint(window, &mut paint);
                let dims = Win32WindowDimension::from(window);
                GLOBAL_BACKBUFFER.win32_copy_buffer_to_window(
                    device_context,
                    dims.width,
                    dims.height,
                );
                let _ = EndPaint(window, &paint);
                LRESULT(0)
            }
            _ => DefWindowProcA(window, message, wparam, lparam),
        }
    }
}
