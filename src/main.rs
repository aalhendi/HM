#![allow(static_mut_refs)]

use std::{mem::size_of, ptr};

use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        Media::Audio::{
            DirectSound::{DSBCAPS_PRIMARYBUFFER, DSBUFFERDESC, DSSCL_PRIORITY, DirectSoundCreate},
            WAVE_FORMAT_PCM, WAVEFORMATEX,
        },
        System::{
            LibraryLoader::GetModuleHandleA,
            Memory::{
                MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree,
            },
        },
        UI::{
            Input::{
                KeyboardAndMouse::{
                    VIRTUAL_KEY, VK_A, VK_D, VK_DOWN, VK_E, VK_ESCAPE, VK_F4, VK_LEFT, VK_Q,
                    VK_RIGHT, VK_S, VK_SPACE, VK_UP, VK_W,
                },
                XboxController::{
                    XINPUT_GAMEPAD_A, XINPUT_GAMEPAD_B, XINPUT_GAMEPAD_BACK,
                    XINPUT_GAMEPAD_DPAD_DOWN, XINPUT_GAMEPAD_DPAD_LEFT, XINPUT_GAMEPAD_DPAD_RIGHT,
                    XINPUT_GAMEPAD_DPAD_UP, XINPUT_GAMEPAD_LEFT_SHOULDER,
                    XINPUT_GAMEPAD_LEFT_THUMB, XINPUT_GAMEPAD_RIGHT_SHOULDER,
                    XINPUT_GAMEPAD_RIGHT_THUMB, XINPUT_GAMEPAD_START, XINPUT_GAMEPAD_X,
                    XINPUT_GAMEPAD_Y, XINPUT_STATE, XInputGetState, XUSER_MAX_COUNT,
                },
            },
            WindowsAndMessaging::*,
        },
    },
    core::*,
};

const BYTES_PER_PIXEL: i32 = 4;
const KEY_MESSAGE_WAS_DOWN_BIT: i32 = 30;
const KEY_MESSAGE_IS_DOWN_BIT: i32 = 31;
const KEY_MESSAGE_IS_ALT_BIT: i32 = 29;

const SAMPLES_PER_SECOND: u32 = 48000;
const BUFFER_SIZE: u32 = 48000 * 2 * 2; // 2 channels, 2 bytes per sample

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
    // NOTE(aalhendi): We store width and height in self.info.bmiHeader. This is redundant. Keeping because its only 8 bytes
    width: i32,
    height: i32,
    pitch: isize,
}

impl Win32OffscreenBuffer {
    unsafe fn render_weird_gradient(&self, blue_offset: i32, green_offset: i32) {
        let width = self.width;
        let height = self.height;

        let mut row = self.memory as *const u8;
        for y in 0..height {
            let mut pixel = row as *mut i32;
            for x in 0..width {
                /*
                Padding is not put first even if its Little Endian because... Windows.
                Memory (u32): BB GG RR XX
                Register: XX RR GG BB where XX is padding 0
                */
                let blue = x.wrapping_add(blue_offset);
                let green = y.wrapping_add(green_offset);

                unsafe {
                    *pixel = (green << 8) | blue;
                    pixel = pixel.offset(1);
                }
            }
            row = unsafe { row.offset(self.pitch) };
        }
    }

    unsafe fn win32_copy_buffer_to_window(&self, device_context: HDC, width: i32, height: i32) {
        // TODO(aalhendi): aspect ratio correction
        // TODO(aalhendi): play with stretch modes
        unsafe {
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
            )
        };
    }

    /// Resize or Initialize a Device Independent Bitmap (DIB)
    unsafe fn win32_resize_dib_section(&mut self, width: i32, height: i32) {
        if self.memory != unsafe { core::mem::zeroed() } {
            let free_res = unsafe { VirtualFree(self.memory, 0, MEM_RELEASE) };
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
        self.memory = unsafe {
            VirtualAlloc(
                None,
                bitmap_memory_size,
                MEM_RESERVE | MEM_COMMIT,
                PAGE_READWRITE,
            )
        };

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
            Some(HINSTANCE::from(module_handle)),
            None,
        )
        .unwrap(); // TODO(aalhendi): use expect(), remove redundant debug assert.

        if window_handle.is_invalid() {
            // TODO(aalhendi): Logging
            // return
        }
        debug_assert!(!window_handle.is_invalid());

        // NOTE(aalhendi): By using CS_OWNDC, can get one device context and keep using it forever since it is not shared.
        let device_context = GetDC(Some(window_handle));
        let mut x_offset = 0;
        let mut y_offset = 0;

        // cant load direct sound till we have a window handle
        let mut ds = None;
        DirectSoundCreate(None, &mut ds, None)?;
        if ds.is_none() {
            panic!("Failed to create direct sound");
        }
        let ds = ds.unwrap();
        ds.SetCooperativeLevel(window_handle, DSSCL_PRIORITY)?;
        let primary_buffer_desc = DSBUFFERDESC {
            dwSize: size_of::<DSBUFFERDESC>() as u32,
            dwFlags: DSBCAPS_PRIMARYBUFFER,
            dwBufferBytes: 0,
            dwReserved: 0,
            // NOTE(aalhendi): we actually can't set the format here, we have to set it later via SetFormat. Windows!
            lpwfxFormat: std::ptr::null_mut(),
            guid3DAlgorithm: GUID::zeroed(),
        };

        let mut primary_buffer = core::mem::zeroed();
        ds.CreateSoundBuffer(&primary_buffer_desc, &mut primary_buffer, None)?;
        if primary_buffer.is_none() {
            panic!("Failed to create primary buffer");
        }
        let primary_buffer = primary_buffer.unwrap();

        // TODO(aalhendi): Move this to a function
        // TODO(aalhendi): make BYTES_PER_SAMPLE and SAMPLES_PER_SECOND variables in the scope of this fn
        let mut wave_format = {
            let n_channels = 2;
            let bits_per_sample = 16;
            let n_block_align = (n_channels * bits_per_sample) / 8;
            let n_samples_per_sec = SAMPLES_PER_SECOND;
            WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_PCM as u16,
                nChannels: n_channels,
                nBlockAlign: n_block_align,
                nSamplesPerSec: n_samples_per_sec,
                nAvgBytesPerSec: n_samples_per_sec * n_block_align as u32,
                wBitsPerSample: bits_per_sample,
                cbSize: 0,
            }
        };
        primary_buffer.SetFormat(&wave_format)?;

        let secondary_buffer_desc = DSBUFFERDESC {
            dwSize: size_of::<DSBUFFERDESC>() as u32,
            dwFlags: 0,
            dwBufferBytes: BUFFER_SIZE,
            dwReserved: 0,
            lpwfxFormat: &mut wave_format,
            guid3DAlgorithm: GUID::zeroed(),
        };

        let mut secondary_buffer = core::mem::zeroed();
        ds.CreateSoundBuffer(&secondary_buffer_desc, &mut secondary_buffer, None)?;
        if secondary_buffer.is_none() {
            panic!("Failed to create secondary buffer");
        }
        // let secondary_buffer = secondary_buffer.unwrap();
        // secondary_buffer.SetFormat(&wave_format)?;

        GLOBAL_RUNNING = true;
        while GLOBAL_RUNNING {
            let mut message = MSG::default();
            while PeekMessageA(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if matches!(message.message, WM_QUIT) {
                    GLOBAL_RUNNING = false;
                }

                DispatchMessageA(&message);
                let _ = TranslateMessage(&message); // TODO(aalhendi): handle zero case?
            }

            // TODO(aalhendi): should we poll this more frequently?
            for controller_index in 0..XUSER_MAX_COUNT {
                let mut controller_state: XINPUT_STATE = XINPUT_STATE::default();
                let x_input_state_res = XInputGetState(controller_index, &mut controller_state);
                if x_input_state_res == ERROR_SUCCESS.0 {
                    // NOTE(aalhendi): This controller is connected
                    // TODO(aalhendi): see if controller_state.dwPacketNumber increments too rapidly
                    let pad = &controller_state.Gamepad;

                    let _up = pad.wButtons & XINPUT_GAMEPAD_DPAD_UP;
                    let _down = pad.wButtons & XINPUT_GAMEPAD_DPAD_DOWN;
                    let _left = pad.wButtons & XINPUT_GAMEPAD_DPAD_LEFT;
                    let _right = pad.wButtons & XINPUT_GAMEPAD_DPAD_RIGHT;
                    let _start = pad.wButtons & XINPUT_GAMEPAD_START;
                    let _back = pad.wButtons & XINPUT_GAMEPAD_BACK;
                    let _left_thumb = pad.wButtons & XINPUT_GAMEPAD_LEFT_THUMB;
                    let _right_thumb = pad.wButtons & XINPUT_GAMEPAD_RIGHT_THUMB;
                    let _left_shoulder = pad.wButtons & XINPUT_GAMEPAD_LEFT_SHOULDER;
                    let _right_shoulder = pad.wButtons & XINPUT_GAMEPAD_RIGHT_SHOULDER;
                    let a_button = pad.wButtons & XINPUT_GAMEPAD_A;
                    let _b_button = pad.wButtons & XINPUT_GAMEPAD_B;
                    let _x_button = pad.wButtons & XINPUT_GAMEPAD_X;
                    let _y_button = pad.wButtons & XINPUT_GAMEPAD_Y;

                    let stick_left_x = pad.sThumbLX;
                    let stick_left_y = pad.sThumbLY;
                    let _stick_right_x = pad.sThumbRX;
                    let _stick_right_y = pad.sThumbRY;

                    x_offset += (stick_left_x as i32) >> 12;
                    y_offset += (stick_left_y as i32) >> 12;

                    if a_button.0 != 0 {
                        y_offset += 2;
                    }
                } else {
                    // NOTE(aalhendi): This controller is not available
                }
            }

            // Test out vibration
            windows::Win32::UI::Input::XboxController::XInputSetState(
                0,
                &windows::Win32::UI::Input::XboxController::XINPUT_VIBRATION {
                    wLeftMotorSpeed: 65535,
                    wRightMotorSpeed: 65535,
                },
            );

            GLOBAL_BACKBUFFER.render_weird_gradient(x_offset, y_offset);

            let dims = Win32WindowDimension::from(window_handle);
            GLOBAL_BACKBUFFER.win32_copy_buffer_to_window(device_context, dims.width, dims.height);
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
            WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP => {
                let virtual_key_code = wparam;
                let was_down = (lparam.0 & (1 << KEY_MESSAGE_WAS_DOWN_BIT)) != 0;
                let is_down = (lparam.0 & (1 << KEY_MESSAGE_IS_DOWN_BIT)) == 0;
                let is_alt_down = (lparam.0 & (1 << KEY_MESSAGE_IS_ALT_BIT)) != 0;
                if was_down != is_down {
                    match VIRTUAL_KEY(virtual_key_code.0 as u16) {
                        VK_W => println!("W key pressed"),
                        VK_S => println!("S key pressed"),
                        VK_A => println!("A key pressed"),
                        VK_D => println!("D key pressed"),
                        VK_Q => println!("Q key pressed"),
                        VK_E => println!("E key pressed"),
                        VK_UP => println!("Up key pressed"),
                        VK_DOWN => println!("Down key pressed"),
                        VK_LEFT => println!("Left key pressed"),
                        VK_RIGHT => println!("Right key pressed"),
                        VK_ESCAPE => println!("Escape key pressed"),
                        VK_SPACE => println!("Space key pressed"),
                        VK_F4 if is_alt_down => {
                            println!("Alt + F4 pressed, quitting...");
                            GLOBAL_RUNNING = false;
                        }
                        _ => {}
                    }
                }
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
