#![allow(static_mut_refs)]

use core::{arch::x86_64, ffi, mem, ptr};

use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        Media::{
            Audio::{
                DirectSound::{
                    DSBCAPS_PRIMARYBUFFER, DSBPLAY_LOOPING, DSBUFFERDESC, DSSCL_PRIORITY,
                    DirectSoundCreate, IDirectSoundBuffer,
                },
                WAVE_FORMAT_PCM, WAVEFORMATEX,
            },
            TIMERR_NOERROR, timeBeginPeriod,
        },
        System::{
            LibraryLoader::GetModuleHandleA,
            Memory::{
                MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree,
            },
            Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
        },
        UI::{
            Input::{
                KeyboardAndMouse::{
                    VIRTUAL_KEY, VK_A, VK_D, VK_DOWN, VK_E, VK_ESCAPE, VK_F4, VK_LEFT, VK_Q,
                    VK_RIGHT, VK_S, VK_SPACE, VK_UP, VK_W,
                },
                XboxController::{
                    XINPUT_GAMEPAD_A, XINPUT_GAMEPAD_B, XINPUT_GAMEPAD_BACK,
                    XINPUT_GAMEPAD_BUTTON_FLAGS, XINPUT_GAMEPAD_DPAD_DOWN,
                    XINPUT_GAMEPAD_DPAD_LEFT, XINPUT_GAMEPAD_DPAD_RIGHT, XINPUT_GAMEPAD_DPAD_UP,
                    XINPUT_GAMEPAD_LEFT_SHOULDER, XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE,
                    XINPUT_GAMEPAD_RIGHT_SHOULDER, XINPUT_GAMEPAD_START, XINPUT_GAMEPAD_X,
                    XINPUT_GAMEPAD_Y, XINPUT_STATE, XInputGetState, XUSER_MAX_COUNT,
                },
            },
            WindowsAndMessaging::*,
        },
    },
    core::*,
};

use hm::{
    GameButton, GameButtonState, GameControllerInput, GameInput, GameMemory, GameOffscreenBuffer,
    GameSoundOutputBuffer, game_update_and_render, gigabytes_to_bytes, megabytes_to_bytes,
};

/* TODO(aalhendi): THIS IS NOT A FINAL PLATFORM LAYER!!!

- Saved game locations
- Getting a handle to our own executable file
- Asset loading path
- Threading (launch a thread)
- Raw Input (support for multiple keyboards)
- Sleep/timeBeginPeriod
- ClipCursor() (for multimonitor support)
- Fullscreen support
- WM_SETCURSOR (control cursor visibility)
- QueryCancelAutoPlay()
- WM_ACTIVATEAPP (for when we are not the active application)
- Blit speed improvements (BitBlt)
- Hardware acceleration (OpenGL, Direct3d or BOTH??)
- GetKeyboardLayout() (for intl WASD support)

*/

const BYTES_PER_PIXEL: i32 = 4;
const KEY_MESSAGE_WAS_DOWN_BIT: i32 = 30;
const KEY_MESSAGE_IS_DOWN_BIT: i32 = 31;
const KEY_MESSAGE_IS_ALT_BIT: i32 = 29;

// TODO(aalhendi): This is a global for now.
static mut GLOBAL_RUNNING: bool = false;
static mut GLOBAL_BACKBUFFER: Win32OffscreenBuffer = Win32OffscreenBuffer {
    info: unsafe { mem::zeroed() }, // alloc'ed in win32_resize_dib_section, called v.early in main fn
    memory: ptr::null_mut(),
    width: 0,
    height: 0,
    pitch: 0,
};

static mut PERF_COUNT_FREQUENCY: i64 = 0;

#[inline(always)]
fn win32_get_wall_clock() -> i64 {
    let mut perf_count = 0;
    unsafe {
        // TODO(aalhendi): handle error
        QueryPerformanceCounter(&mut perf_count).unwrap();
        perf_count
    }
}

#[inline(always)]
// NOTE(aalhendi): 32bit? im making this f64 for now.
fn win32_get_seconds_elapsed(start: i64, end: i64) -> f64 {
    (end - start) as f64 / unsafe { PERF_COUNT_FREQUENCY } as f64
}

// NOTE(aalhendi): we return the ds, primary_buffer, secondary_buffer to avoid them being dropped.
// we're not making them global for now
fn win32_init_dsound(
    window_handle: HWND,
    sound_output: &mut Win32SoundOutput,
) -> windows::core::Result<(
    windows::Win32::Media::Audio::DirectSound::IDirectSound,
    IDirectSoundBuffer,
    IDirectSoundBuffer,
)> {
    let mut ds = None;
    unsafe {
        DirectSoundCreate(None, &mut ds, None)?;
    }
    if ds.is_none() {
        panic!("Failed to create direct sound");
    }
    let ds = ds.unwrap();
    unsafe {
        ds.SetCooperativeLevel(window_handle, DSSCL_PRIORITY)?;
    }
    let primary_buffer_desc = DSBUFFERDESC {
        dwSize: size_of::<DSBUFFERDESC>() as u32,
        dwFlags: DSBCAPS_PRIMARYBUFFER,
        dwBufferBytes: 0,
        dwReserved: 0,
        // NOTE(aalhendi): we actually can't set the format here, we have to set it later via SetFormat. Windows!
        lpwfxFormat: ptr::null_mut(),
        guid3DAlgorithm: GUID::zeroed(),
    };

    let mut primary_buffer = None;
    unsafe {
        ds.CreateSoundBuffer(&primary_buffer_desc, &mut primary_buffer, None)?;
    }
    if primary_buffer.is_none() {
        panic!("Failed to create primary buffer");
    }
    let primary_buffer = primary_buffer.unwrap();

    let mut wave_format = {
        let n_channels = 2;
        let n_block_align = (n_channels * 16) / 8; // TODO(aalhendi): remove hardcoded 16, use sound_output.bytes_per_sample?
        WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: n_channels,
            nBlockAlign: n_block_align,
            nSamplesPerSec: sound_output.samples_per_second,
            nAvgBytesPerSec: sound_output.samples_per_second * n_block_align as u32,
            wBitsPerSample: 16, // TODO(aalhendi): remove hardcoded 16, use sound_output.bytes_per_sample?
            cbSize: 0,
        }
    };
    unsafe {
        primary_buffer.SetFormat(&wave_format)?;
    }

    let secondary_buffer_desc = DSBUFFERDESC {
        dwSize: size_of::<DSBUFFERDESC>() as u32,
        dwFlags: 0,
        dwBufferBytes: sound_output.buffer_size,
        dwReserved: 0,
        lpwfxFormat: &mut wave_format,
        guid3DAlgorithm: GUID::zeroed(),
    };

    let mut secondary_buffer = None;
    unsafe {
        ds.CreateSoundBuffer(&secondary_buffer_desc, &mut secondary_buffer, None)?;
    }
    if secondary_buffer.is_none() {
        panic!("Failed to create secondary buffer");
    }
    let secondary_buffer = secondary_buffer.unwrap();

    Ok((ds, primary_buffer, secondary_buffer))
}

struct Win32SoundOutput {
    samples_per_second: u32,
    running_sample_index: u32,
    bytes_per_sample: u32,
    buffer_size: u32,
    latency_sample_count: u32,
}

impl Win32SoundOutput {
    fn new(samples_per_second: u32) -> Self {
        let bytes_per_sample = size_of::<i16>() as u32 * 2;

        Self {
            samples_per_second,
            running_sample_index: 0,
            bytes_per_sample,
            buffer_size: samples_per_second * bytes_per_sample, // 2 channels, 2 bytes per sample
            latency_sample_count: samples_per_second / 15,
        }
    }
}

fn win32_clear_sound_buffer(
    secondary_buffer: &IDirectSoundBuffer,
    sound_output: &mut Win32SoundOutput,
) -> windows::core::Result<()> {
    // To be filled by Lock:
    let mut region1_ptr: *mut ffi::c_void = ptr::null_mut();
    let mut region1_size: u32 = 0;
    let mut region2_ptr: *mut ffi::c_void = ptr::null_mut();
    let mut region2_size: u32 = 0;
    unsafe {
        secondary_buffer.Lock(
            0,
            sound_output.buffer_size,
            &mut region1_ptr,
            &mut region1_size,
            Some(&mut region2_ptr),
            Some(&mut region2_size),
            0,
        )?;
    }

    let mut dest_sample = region1_ptr as *mut u8;
    for _byte_index in 0..region1_size {
        unsafe {
            *dest_sample = 0;
            dest_sample = dest_sample.offset(1);
        }
    }

    // NOTE(aalhendi): region2 is not null if we are wrapping around the ring buffer... and we expect a contiguous region at startup so this will rarely be hit.
    dest_sample = region2_ptr as *mut u8;
    for _byte_index in 0..region2_size {
        unsafe {
            *dest_sample = 0;
            dest_sample = dest_sample.offset(1);
        }
    }

    unsafe {
        secondary_buffer.Unlock(region1_ptr, region1_size, None, 0)?;
    }

    Ok(())
}

fn win32_fill_sound_buffer(
    // note(aalhendi): this is meant to be a "global" secondary buffer, but we are passing it in as an argument for now
    secondary_buffer: &IDirectSoundBuffer,
    sound_output: &mut Win32SoundOutput,
    bytes_to_lock: u32,
    bytes_to_write: u32,
    source_buffer: &mut GameSoundOutputBuffer,
) -> windows::core::Result<()> {
    // To be filled by Lock:
    let mut region1_ptr: *mut ffi::c_void = ptr::null_mut();
    let mut region1_size: u32 = 0;
    let mut region2_ptr: *mut ffi::c_void = ptr::null_mut();
    let mut region2_size: u32 = 0;

    unsafe {
        secondary_buffer.Lock(
            bytes_to_lock,
            bytes_to_write,
            &mut region1_ptr,
            &mut region1_size,
            Some(&mut region2_ptr),
            Some(&mut region2_size),
            0,
        )?;
    }

    // TODO(aalhendi): assert that region1_size , region2_size are valid

    let region1_sample_count = region1_size / sound_output.bytes_per_sample;
    let mut dest_sample = region1_ptr as *mut i16;
    let mut source_sample = source_buffer.samples;
    for _sample_index in 0..region1_sample_count {
        // basically, we write L/R L/R L/R L/R etc.
        // we use sample_out as an i16 ptr to the memory location we want to write to (region1 / ringbuffer)
        unsafe {
            *dest_sample = *source_sample;
            source_sample = source_sample.offset(1);
            dest_sample = dest_sample.offset(1);
            *dest_sample = *source_sample;
            source_sample = source_sample.offset(1);
            dest_sample = dest_sample.offset(1);
        }
        sound_output.running_sample_index = sound_output.running_sample_index.overflowing_add(1).0;
    }

    // in the case where we are wrapping around the ring buffer, we need to fill region2
    // todo(aalhendi): same loop as above, but for region2, can we collapse the 2 loops?
    if !region2_ptr.is_null() {
        let region2_sample_count = region2_size / sound_output.bytes_per_sample;
        dest_sample = region2_ptr as *mut i16;
        for _sample_index in 0..region2_sample_count {
            unsafe {
                *dest_sample = *source_sample;
                source_sample = source_sample.offset(1);
                dest_sample = dest_sample.offset(1);
                *dest_sample = *source_sample;
                source_sample = source_sample.offset(1);
                dest_sample = dest_sample.offset(1);
            }
            sound_output.running_sample_index =
                sound_output.running_sample_index.overflowing_add(1).0;
        }
    }

    // for some reason, unlock expects a different signature than lock... so we have to do this
    // refactor(aalhendi): we can have 2 unlock calls, one running if region2_ptr is null, and the other running if it is not null after the write
    let (final_ptr_region2, final_size_region2) = if region2_ptr.is_null() {
        (None, 0)
    } else {
        (Some(region2_ptr as *const ffi::c_void), region2_size)
    };

    unsafe {
        secondary_buffer.Unlock(
            region1_ptr as *const ffi::c_void,
            region1_size,
            final_ptr_region2,
            final_size_region2,
        )
    }
}

fn win32_process_keyboard_message(new_state: &mut GameButtonState, is_down: bool) {
    debug_assert!(
        new_state.ended_down != is_down,
        "Button state is already in the desired state. We should hit this if the state changed."
    );
    new_state.ended_down = is_down;
    new_state.half_transition_count += 1;
}

fn win32_process_x_input_digital_button(
    x_input_button_state: XINPUT_GAMEPAD_BUTTON_FLAGS,
    old_state: &mut GameButtonState,
    button_bit: XINPUT_GAMEPAD_BUTTON_FLAGS,
    new_state: &mut GameButtonState,
) {
    new_state.ended_down = (x_input_button_state & button_bit) == button_bit;
    new_state.half_transition_count = if old_state.ended_down != new_state.ended_down {
        1
    } else {
        0
    };
}

fn win32_process_x_input_stick_value(value: i16, deadzone_threshold: i16) -> f32 {
    let dz_f32 = deadzone_threshold as f32;
    if value < -deadzone_threshold {
        (value as f32 + dz_f32) / (32768_f32 - dz_f32)
    } else if value > deadzone_threshold {
        (value as f32 - dz_f32) / (32767_f32 - dz_f32)
    } else {
        0_f32
    }
}

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
    memory: *mut ffi::c_void,
    // NOTE(aalhendi): We store width and height in self.info.bmiHeader. This is redundant. Keeping because its only 8 bytes
    width: i32,
    height: i32,
    pitch: isize,
}

impl Win32OffscreenBuffer {
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
        if self.memory != unsafe { mem::zeroed() } {
            let free_res = unsafe { VirtualFree(self.memory, 0, MEM_RELEASE) };
            if let Err(e) = free_res {
                panic!("{e}");
            }
        }

        self.width = width;
        self.height = height;

        let bitmap_info_header = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: self.width,
            // NOTE(aalhendi): When bHeight is negative, it clues Windows to treat the bitmap as top-down rather than bottom-up. This means tht the first three bytes are for the top-left pixel.
            biHeight: -self.height,
            biPlanes: 1,
            biBitCount: 32, // 8 for red, 8 for green, 8 for blue, ask for 32 for DWORD alignment
            biCompression: BI_RGB.0, // Uncompressed
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };

        self.info.bmiHeader = bitmap_info_header;

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

        QueryPerformanceFrequency(&mut PERF_COUNT_FREQUENCY)?;

        // NOTE(aalhendi): Set windows scheduler granularity. This is used to make our sleep more accurate (granular).
        let desired_scheduler_ms = 1;
        let sleep_is_granular = timeBeginPeriod(desired_scheduler_ms) == TIMERR_NOERROR;
        if !sleep_is_granular {
            println!("Sleep is not granular. This is bad.");
        }

        GLOBAL_BACKBUFFER.win32_resize_dib_section(1280, 720);

        let wc = WNDCLASSA {
            style: CS_HREDRAW | CS_VREDRAW | CS_OWNDC,
            lpfnWndProc: Some(win32_main_window_callback),
            hInstance: GetModuleHandleA(None)?.into(),
            lpszClassName: s!("HandmadeHeroWindowClass"),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };

        // TODO(aalhendi): GetSystemMetrics(SM_SAMPLERATE)? How do we reliably query refresh rate? GetComposition?
        let monitor_refresh_hz = 60_i32;
        let game_update_hz = monitor_refresh_hz / 2;
        let target_seconds_per_frame = 1_f64 / game_update_hz as f64;

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

        // NOTE(aalhendi): Audio test
        // TODO(aalhendi): make this sixty seconds?
        let mut sound_output = Win32SoundOutput::new(48000);

        // cant load direct sound till we have a window handle
        let (_ds, _primary_buffer, secondary_buffer) =
            win32_init_dsound(window_handle, &mut sound_output)?;

        win32_clear_sound_buffer(&secondary_buffer, &mut sound_output)?;
        secondary_buffer.Play(0, 0, DSBPLAY_LOOPING)?;

        GLOBAL_RUNNING = true;

        // TODO(aalhendi): pool with bitmap VirtualAlloc
        let samples = VirtualAlloc(
            None,
            sound_output.buffer_size as usize,
            MEM_RESERVE | MEM_COMMIT,
            PAGE_READWRITE,
        ) as *mut i16;

        let permanent_storage_size = megabytes_to_bytes(64);
        let transient_storage_size = gigabytes_to_bytes(4);
        // TODO(aalhendi): handle various memory footprints (USING SYSTEM METRICS)
        let total_storage_size = permanent_storage_size + transient_storage_size;
        let base_address = {
            #[cfg(feature = "internal_build")]
            {
                use hm::terabytes_to_bytes;
                terabytes_to_bytes(2)
            }
            #[cfg(not(feature = "internal_build"))]
            {
                0_usize
            }
        };

        let permanent_storage = VirtualAlloc(
            Some(base_address as *mut ffi::c_void),
            total_storage_size,
            MEM_RESERVE | MEM_COMMIT,
            PAGE_READWRITE,
        ) as *mut ();

        let mut game_memory = GameMemory {
            is_initialized: false,
            permanent_storage_size,
            transient_storage_size,
            permanent_storage,
            transient_storage: permanent_storage
                .cast::<u8>()
                .add(permanent_storage_size)
                .cast::<()>(),
        };

        if samples.is_null()
            || game_memory.permanent_storage.is_null()
            || game_memory.transient_storage.is_null()
        {
            panic!("Failed to allocate samples or permanent storage");
        }

        let mut input = [GameInput::default(), GameInput::default()];
        // NOTE(aalhendi): this is a hack to get around the fact that we cant have 2 mutable references to the same array
        let (new_input_slice, old_input_slice) = input.split_at_mut(1);

        let mut new_input = &mut new_input_slice[0];
        let mut old_input = &mut old_input_slice[0];

        let mut last_counter = win32_get_wall_clock();
        // TODO(aalhendi): do we want to use rdtscp instead?
        let mut last_cycle_count = x86_64::_rdtsc();

        while GLOBAL_RUNNING {
            let new_keyboard_controller = &mut new_input.controllers[0];
            let old_keyboard_controller = &mut old_input.controllers[0];
            // TODO(aalhendi): we can't zero everything because the up/down count will be wrong
            *new_keyboard_controller = GameControllerInput::default();
            new_keyboard_controller.is_connected = true;
            for (i, button) in new_keyboard_controller.buttons.iter_mut().enumerate() {
                button.ended_down = old_keyboard_controller.buttons[i].ended_down;
            }

            win32_process_pending_messages(new_keyboard_controller);

            // TODO(aalhendi): should we poll this more frequently?
            // TODO(aalhendi): need to not poll disconnected controllers to avoid xinput frame hit on older libraries
            let mut max_controller_count = XUSER_MAX_COUNT;
            if max_controller_count as usize > new_input.controllers.len() {
                max_controller_count = new_input.controllers.len() as u32;
            }
            // NOTE(aalhendi): max controller count minus the keyboard controller
            for controller_index in 0..max_controller_count - 1 {
                let old_controller = &mut old_input.controllers[controller_index as usize];
                let new_controller = &mut new_input.controllers[controller_index as usize];

                let mut controller_state: XINPUT_STATE = XINPUT_STATE::default();
                let x_input_state_res = XInputGetState(controller_index, &mut controller_state);
                if x_input_state_res == ERROR_SUCCESS.0 {
                    new_controller.is_connected = true;

                    // NOTE(aalhendi): This controller is connected
                    // TODO(aalhendi): see if controller_state.dwPacketNumber increments too rapidly
                    let pad = &controller_state.Gamepad;

                    let stick_left_x = win32_process_x_input_stick_value(
                        pad.sThumbLX,
                        XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE.0 as i16,
                    );
                    new_controller.left_stick_average_x = stick_left_x;

                    let stick_left_y = win32_process_x_input_stick_value(
                        pad.sThumbLY,
                        XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE.0 as i16,
                    );
                    new_controller.left_stick_average_y = stick_left_y;

                    if stick_left_x != 0.0 || stick_left_y != 0.0 {
                        new_controller.is_analog = true;
                    }

                    // TODO(aalhendi): add right stick support

                    if (pad.wButtons & XINPUT_GAMEPAD_DPAD_UP).0 != 0 {
                        new_controller.is_analog = false;
                        new_controller.left_stick_average_y = 1.0;
                    }
                    if (pad.wButtons & XINPUT_GAMEPAD_DPAD_DOWN).0 != 0 {
                        new_controller.is_analog = false;
                        new_controller.left_stick_average_y = -1.0;
                    }
                    if (pad.wButtons & XINPUT_GAMEPAD_DPAD_LEFT).0 != 0 {
                        new_controller.is_analog = false;
                        new_controller.left_stick_average_x = 1.0;
                    }
                    if (pad.wButtons & XINPUT_GAMEPAD_DPAD_RIGHT).0 != 0 {
                        new_controller.is_analog = false;
                        new_controller.left_stick_average_x = -1.0;
                    }

                    let threshold = 0.5_f32;
                    // NOTE(aalhendi): fake dpad emulation from left stick
                    win32_process_x_input_digital_button(
                        XINPUT_GAMEPAD_BUTTON_FLAGS(
                            (new_controller.left_stick_average_x < -threshold) as u16,
                        ),
                        old_controller.button_mut(GameButton::MoveLeft),
                        XINPUT_GAMEPAD_DPAD_LEFT,
                        new_controller.button_mut(GameButton::MoveLeft),
                    );
                    win32_process_x_input_digital_button(
                        XINPUT_GAMEPAD_BUTTON_FLAGS(
                            (new_controller.left_stick_average_x > threshold) as u16,
                        ),
                        old_controller.button_mut(GameButton::MoveRight),
                        XINPUT_GAMEPAD_DPAD_RIGHT,
                        new_controller.button_mut(GameButton::MoveRight),
                    );
                    win32_process_x_input_digital_button(
                        XINPUT_GAMEPAD_BUTTON_FLAGS(
                            (new_controller.left_stick_average_y < -threshold) as u16,
                        ),
                        old_controller.button_mut(GameButton::MoveUp),
                        XINPUT_GAMEPAD_DPAD_UP,
                        new_controller.button_mut(GameButton::MoveUp),
                    );
                    win32_process_x_input_digital_button(
                        XINPUT_GAMEPAD_BUTTON_FLAGS(
                            (new_controller.left_stick_average_y > threshold) as u16,
                        ),
                        old_controller.button_mut(GameButton::MoveDown),
                        XINPUT_GAMEPAD_DPAD_DOWN,
                        new_controller.button_mut(GameButton::MoveDown),
                    );

                    win32_process_x_input_digital_button(
                        pad.wButtons,
                        old_controller.button_mut(GameButton::ActionDown),
                        XINPUT_GAMEPAD_A,
                        new_controller.button_mut(GameButton::ActionDown),
                    );

                    win32_process_x_input_digital_button(
                        pad.wButtons,
                        old_controller.button_mut(GameButton::ActionRight),
                        XINPUT_GAMEPAD_B,
                        new_controller.button_mut(GameButton::ActionRight),
                    );

                    win32_process_x_input_digital_button(
                        pad.wButtons,
                        old_controller.button_mut(GameButton::ActionLeft),
                        XINPUT_GAMEPAD_X,
                        new_controller.button_mut(GameButton::ActionLeft),
                    );

                    win32_process_x_input_digital_button(
                        pad.wButtons,
                        old_controller.button_mut(GameButton::ActionUp),
                        XINPUT_GAMEPAD_Y,
                        new_controller.button_mut(GameButton::ActionUp),
                    );

                    win32_process_x_input_digital_button(
                        pad.wButtons,
                        old_controller.button_mut(GameButton::LeftShoulder),
                        XINPUT_GAMEPAD_LEFT_SHOULDER,
                        new_controller.button_mut(GameButton::LeftShoulder),
                    );

                    win32_process_x_input_digital_button(
                        pad.wButtons,
                        old_controller.button_mut(GameButton::RightShoulder),
                        XINPUT_GAMEPAD_RIGHT_SHOULDER,
                        new_controller.button_mut(GameButton::RightShoulder),
                    );

                    win32_process_x_input_digital_button(
                        pad.wButtons,
                        old_controller.button_mut(GameButton::Start),
                        XINPUT_GAMEPAD_START,
                        new_controller.button_mut(GameButton::Start),
                    );

                    win32_process_x_input_digital_button(
                        pad.wButtons,
                        old_controller.button_mut(GameButton::Back),
                        XINPUT_GAMEPAD_BACK,
                        new_controller.button_mut(GameButton::Back),
                    );
                } else {
                    // NOTE(aalhendi): This controller is not available
                    new_controller.is_connected = false;
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

            // offset, in bytes, of the play cursor
            let mut play_cursor = 0u32;
            // offset, in bytes, of the write cursor
            let mut write_cursor = 0u32;
            let mut is_sound_valid = false;
            let mut bytes_to_lock = 0u32;
            let mut bytes_to_write = 0u32;
            // TODO(aalhendi): tighten up sound logic so that we know where we should be writing to and we can anticipate the time spent in the game update
            match secondary_buffer
                .GetCurrentPosition(Some(&mut play_cursor), Some(&mut write_cursor))
            {
                Ok(_) => {
                    // lock offset
                    bytes_to_lock = sound_output
                        .running_sample_index
                        .overflowing_mul(sound_output.bytes_per_sample)
                        .0
                        % sound_output.buffer_size;

                    // lock size
                    let target_write_cursor = (play_cursor
                        + (sound_output.latency_sample_count * sound_output.bytes_per_sample))
                        % sound_output.buffer_size;
                    bytes_to_write = if bytes_to_lock > target_write_cursor {
                        // case 1: we are wrapping around the ring buffer, fill 2 regions
                        (sound_output.buffer_size - bytes_to_lock) + target_write_cursor
                    } else {
                        // case 2: we are not wrapping around the ring buffer, fill 1 region
                        target_write_cursor - bytes_to_lock
                    };

                    is_sound_valid = true
                }
                Err(e) => println!("Error getting sound position: {e:?}"),
            }

            // TODO(aalhendi): sound buffer is wrong now because its not updated to work with new frame loop
            let mut sound_buffer = GameSoundOutputBuffer {
                samples_per_second: sound_output.samples_per_second,
                sample_count: bytes_to_write / sound_output.bytes_per_sample,
                samples,
            };

            let mut buffer = GameOffscreenBuffer {
                width: GLOBAL_BACKBUFFER.width,
                height: GLOBAL_BACKBUFFER.height,
                pitch: GLOBAL_BACKBUFFER.pitch,
                memory: GLOBAL_BACKBUFFER.memory,
            };

            game_update_and_render(&mut game_memory, new_input, &mut buffer, &mut sound_buffer);

            if is_sound_valid {
                // NOTE(aalhendi): ideally, we want to only fill sound buffer if there's something to write.
                // this fn calls Lock(), which can fail if bytes_to_write is 0
                // we are ignoring the error for now, skipping this frame if it fails. This is to match C behavior.
                // from testing, this only happens the first time call occurs. we could have a if check to see if bytes_to_write is 0
                // but that check would run every frame, which is not ideal.
                if let Err(e) = win32_fill_sound_buffer(
                    &secondary_buffer,
                    &mut sound_output,
                    bytes_to_lock,
                    bytes_to_write,
                    &mut sound_buffer,
                ) {
                    println!("Error filling sound buffer: {e:?}");
                }
            }

            // TODO(aalhendi): NOT TESTED YET!
            let work_counter = win32_get_wall_clock();
            let work_seconds_elapsed = win32_get_seconds_elapsed(last_counter, work_counter);

            let mut seconds_elapsed_for_frame = work_seconds_elapsed;
            if seconds_elapsed_for_frame < target_seconds_per_frame {
                while seconds_elapsed_for_frame < target_seconds_per_frame {
                    if sleep_is_granular {
                        let sleep_ms =
                            (target_seconds_per_frame - seconds_elapsed_for_frame) * 1000_f64;
                        if sleep_ms > 1_f64 {
                            // TODO(aalhendi): sleeping is hard... see Intel's TPAUSE instruction. I think AMD uses UMWAIT.
                            windows::Win32::System::Threading::Sleep(sleep_ms as u32);
                        }
                    }

                    #[cfg(feature = "internal_build")]
                    {
                        let test_seconds_elapsed_for_frame =
                            win32_get_seconds_elapsed(last_counter, win32_get_wall_clock());
                        const FRAME_TIME_SLOP_S: f64 = 0.002;
                        debug_assert!(
                            test_seconds_elapsed_for_frame
                                < target_seconds_per_frame + FRAME_TIME_SLOP_S,
                            "Test seconds elapsed for frame is greater than target seconds per frame {test_seconds_elapsed_for_frame} > {target_seconds_per_frame}"
                        );
                    }

                    seconds_elapsed_for_frame =
                        win32_get_seconds_elapsed(last_counter, win32_get_wall_clock());
                }
            } else {
                // TODO(aalhendi): handle missed frame
                println!(
                    "MISSED TARGET FPS!!! {seconds_elapsed_for_frame} < {target_seconds_per_frame}"
                );
            }

            let dims = Win32WindowDimension::from(window_handle);
            GLOBAL_BACKBUFFER.win32_copy_buffer_to_window(device_context, dims.width, dims.height);

            mem::swap(&mut new_input, &mut old_input);
            // TODO(aalhendi): should i clear these here?

            let end_counter = win32_get_wall_clock();
            let ms_per_frame = 1_000_f64 * win32_get_seconds_elapsed(last_counter, end_counter);
            last_counter = end_counter;

            let end_cycle_count = x86_64::_rdtsc();
            let cycles_elapsed = end_cycle_count as f64 - last_cycle_count as f64;
            last_cycle_count = end_cycle_count;

            let fps = 0_f64; // TODO(aalhendi): calculate fps
            println!(
                "{ms_per_frame:.2} ms/frame - {fps:.1} fps - {mc:.2} mega_cycles/frame",
                mc = cycles_elapsed as f64 / (1_000_f64 * 1_000_f64)
            );
        }

        Ok(())
    }
}

unsafe fn win32_process_pending_messages(keyboard_controller: &mut GameControllerInput) {
    let mut message = MSG::default();
    while unsafe { PeekMessageA(&mut message, None, 0, 0, PM_REMOVE).as_bool() } {
        match message.message {
            WM_QUIT => unsafe {
                GLOBAL_RUNNING = false;
            },
            WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP => {
                let virtual_key_code = message.wParam;
                let was_down = (message.lParam.0 & (1 << KEY_MESSAGE_WAS_DOWN_BIT)) != 0;
                let is_down = (message.lParam.0 & (1 << KEY_MESSAGE_IS_DOWN_BIT)) == 0;
                let is_alt_down = (message.lParam.0 & (1 << KEY_MESSAGE_IS_ALT_BIT)) != 0;
                if was_down != is_down {
                    match VIRTUAL_KEY(virtual_key_code.0 as u16) {
                        VK_W => win32_process_keyboard_message(
                            keyboard_controller.button_mut(GameButton::MoveUp),
                            is_down,
                        ),
                        VK_S => win32_process_keyboard_message(
                            keyboard_controller.button_mut(GameButton::MoveDown),
                            is_down,
                        ),
                        VK_A => win32_process_keyboard_message(
                            keyboard_controller.button_mut(GameButton::MoveLeft),
                            is_down,
                        ),
                        VK_D => win32_process_keyboard_message(
                            keyboard_controller.button_mut(GameButton::MoveRight),
                            is_down,
                        ),
                        VK_Q => win32_process_keyboard_message(
                            keyboard_controller.button_mut(GameButton::LeftShoulder),
                            is_down,
                        ),
                        VK_E => win32_process_keyboard_message(
                            keyboard_controller.button_mut(GameButton::RightShoulder),
                            is_down,
                        ),
                        VK_UP => win32_process_keyboard_message(
                            keyboard_controller.button_mut(GameButton::ActionUp),
                            is_down,
                        ),
                        VK_DOWN => win32_process_keyboard_message(
                            keyboard_controller.button_mut(GameButton::ActionDown),
                            is_down,
                        ),
                        VK_LEFT => win32_process_keyboard_message(
                            keyboard_controller.button_mut(GameButton::ActionLeft),
                            is_down,
                        ),
                        VK_RIGHT => win32_process_keyboard_message(
                            keyboard_controller.button_mut(GameButton::ActionRight),
                            is_down,
                        ),
                        VK_ESCAPE => win32_process_keyboard_message(
                            keyboard_controller.button_mut(GameButton::Start),
                            is_down,
                        ),
                        VK_SPACE => win32_process_keyboard_message(
                            keyboard_controller.button_mut(GameButton::Back),
                            is_down,
                        ),
                        VK_F4 if is_alt_down => {
                            println!("Alt + F4 pressed, quitting...");
                            unsafe {
                                GLOBAL_RUNNING = false;
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => unsafe {
                let _ = TranslateMessage(&message); // TODO(aalhendi): handle zero case?
                DispatchMessageA(&message);
            },
        }
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
                debug_assert!(
                    false,
                    "Keyboard input came in through a non-dispatch event. This should not happen. Means it likely came through via callback."
                );
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
