#![allow(static_mut_refs)]

use core::{arch::x86_64, ffi, mem, ptr};
use interface::{
    GameButton, GameButtonState, GameControllerInput, GameInput, GameMemory, GameOffscreenBuffer,
    GameSoundOutputBuffer, gigabytes_to_bytes, megabytes_to_bytes,
};

#[cfg(feature = "internal_build")]
use interface::DebugPlatformReadFileResult;

use windows_sys::{
    Win32::{
        Foundation::{
            COLORREF, CloseHandle, ERROR_SUCCESS, FALSE, FARPROC, FILETIME, FreeLibrary,
            GENERIC_READ, GENERIC_WRITE, HANDLE, HINSTANCE, HMODULE, HWND, INVALID_HANDLE_VALUE,
            LPARAM, LRESULT, MAX_PATH, RECT, TRUE, WPARAM,
        },
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, DIB_RGB_COLORS, EndPaint, GetDC, HDC,
            PAINTSTRUCT, ReleaseDC, SRCCOPY, StretchDIBits,
        },
        Media::{
            Audio::{WAVE_FORMAT_PCM, WAVEFORMATEX},
            TIMERR_NOERROR, timeBeginPeriod,
        },
        Storage::FileSystem::{
            CREATE_ALWAYS, CompareFileTime, CopyFileA, CreateFileA, FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_NONE, FILE_SHARE_READ, GetFileAttributesExA, GetFileExInfoStandard,
            OPEN_EXISTING, ReadFile, WIN32_FILE_ATTRIBUTE_DATA, WriteFile,
        },
        System::{
            LibraryLoader::{GetModuleFileNameA, GetModuleHandleA, GetProcAddress, LoadLibraryA},
            Memory::{
                MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
                VirtualAlloc, VirtualFree,
            },
            Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
        },
        UI::{
            Input::{
                KeyboardAndMouse::{
                    VIRTUAL_KEY, VK_A, VK_D, VK_DOWN, VK_E, VK_ESCAPE, VK_F4, VK_L, VK_LEFT, VK_P,
                    VK_Q, VK_RIGHT, VK_S, VK_SPACE, VK_UP, VK_W,
                },
                XboxController::{
                    XINPUT_GAMEPAD_A, XINPUT_GAMEPAD_B, XINPUT_GAMEPAD_BACK,
                    XINPUT_GAMEPAD_BUTTON_FLAGS, XINPUT_GAMEPAD_DPAD_DOWN,
                    XINPUT_GAMEPAD_DPAD_LEFT, XINPUT_GAMEPAD_DPAD_RIGHT, XINPUT_GAMEPAD_DPAD_UP,
                    XINPUT_GAMEPAD_LEFT_SHOULDER, XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE,
                    XINPUT_GAMEPAD_RIGHT_SHOULDER, XINPUT_GAMEPAD_START, XINPUT_GAMEPAD_X,
                    XINPUT_GAMEPAD_Y, XINPUT_STATE, XINPUT_VIBRATION, XInputGetState,
                    XInputSetState, XUSER_MAX_COUNT,
                },
            },
            WindowsAndMessaging::*,
        },
    },
    core::{GUID, PCSTR, s},
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

// NOTE(aalhendi): Never use MAX_PATH in code that is user-facing because it can be dangerous and lead to bad resutls.
const MAX_PATH_USIZE: usize = MAX_PATH as usize;
const KEY_MESSAGE_WAS_DOWN_BIT: i32 = 30;
const KEY_MESSAGE_IS_DOWN_BIT: i32 = 31;
const KEY_MESSAGE_IS_ALT_BIT: i32 = 29;

// TODO(aalhendi): GetSystemMetrics(SM_SAMPLERATE)? How do we reliably query refresh rate? GetComposition?
const MONITOR_REFRESH_HZ: i32 = 60;
const GAME_UPDATE_HZ: i32 = MONITOR_REFRESH_HZ / 2;
const TARGET_SECONDS_PER_FRAME: f64 = 1_f64 / GAME_UPDATE_HZ as f64;

// TODO(aalhendi): This is a global for now.
static mut GLOBAL_RUNNING: bool = false;
static mut GLOBAL_PAUSE: bool = false;
static mut GLOBAL_BACKBUFFER: Win32OffscreenBuffer = Win32OffscreenBuffer {
    info: unsafe { mem::zeroed() }, // alloc'ed in win32_resize_dib_section, called v.early in the main fn
    memory: ptr::null_mut(),
    width: 0,
    height: 0,
    pitch: 0,
    bytes_per_pixel: 4,
};

static mut PERF_COUNT_FREQUENCY: i64 = 0;

pub type GameUpdateAndRenderFn = unsafe extern "C" fn(
    memory: &mut GameMemory,
    input: &mut GameInput,
    buffer: &mut GameOffscreenBuffer,
);

pub type GameGetSoundSamplesFn =
    unsafe extern "C" fn(memory: &mut GameMemory, sound_buffer: &mut GameSoundOutputBuffer);

/// A helper function to create a COLORREF from RGB values. `windows-sys` doesn't have the equivalent of the C macro `RGB`.
#[inline(always)]
const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

#[repr(transparent)]
struct Win32Path([u8; MAX_PATH_USIZE]);

impl Win32Path {
    fn as_ptr(&self) -> *const u8 {
        self.0.as_ptr()
    }
}

struct Win32GameCode {
    // Windows DLL handle (we'll need this to unload it later)
    pub game_code_dll: HMODULE,
    // The last time the DLL was modified
    pub last_write_time: FILETIME,

    // The function pointers
    pub update_and_render: Option<GameUpdateAndRenderFn>,
    pub get_sound_samples: Option<GameGetSoundSamplesFn>,

    pub is_valid: bool,
}

struct Win32State {
    total_size: usize,
    game_memory_block: *mut (),

    recording_file_handle: HANDLE,
    input_playing_idx: u32,

    input_recording_idx: u32,
    playback_file_handle: HANDLE,

    pub exe_file_name: [u8; MAX_PATH_USIZE],
    pub exe_file_name_base_offset: usize,
}

impl Default for Win32State {
    fn default() -> Self {
        Self {
            total_size: 0,
            game_memory_block: ptr::null_mut(),
            recording_file_handle: INVALID_HANDLE_VALUE,
            input_playing_idx: 0,
            input_recording_idx: 0,
            playback_file_handle: INVALID_HANDLE_VALUE,
            exe_file_name: [0; MAX_PATH as usize],
            exe_file_name_base_offset: 0,
        }
    }
}

impl Win32State {
    fn win32_get_exe_file_name(&mut self, module_handle: HMODULE) {
        let buffer_ptr = self.exe_file_name.as_mut_ptr();
        let buffer_len = self.exe_file_name.len() as u32;
        let file_name_size = unsafe { GetModuleFileNameA(module_handle, buffer_ptr, buffer_len) };

        // NOTE(aalhendi): check for failure or partial path. They are both useless.
        if file_name_size == 0 || file_name_size >= buffer_len {
            eprintln!("Error: GetModuleFileNameA failed");
            self.exe_file_name_base_offset = 0;
            return;
        }

        if let Some(pos) = self.exe_file_name[..file_name_size as usize]
            .iter()
            .rposition(|&c| c == b'\\' || c == b'/')
        {
            self.exe_file_name_base_offset = pos + 1;
        } else {
            self.exe_file_name_base_offset = 0;
        }
    }

    fn win32_build_exe_path_file_name(&self, file_name: &ffi::CStr, dest: &mut Win32Path) {
        let base_len = self.exe_file_name_base_offset;
        let file_bytes = file_name.to_bytes_with_nul();

        debug_assert!(
            base_len + file_bytes.len() <= dest.0.len(),
            "Path buffer too small!"
        );

        dest.0[..base_len].copy_from_slice(&self.exe_file_name[..base_len]);

        let end_idx = base_len + file_bytes.len();
        dest.0[base_len..end_idx].copy_from_slice(file_bytes);
    }
}

#[inline]
fn win32_get_last_write_time(file_name: PCSTR) -> FILETIME {
    let mut data = WIN32_FILE_ATTRIBUTE_DATA::default();
    let data_ptr = &mut data as *mut WIN32_FILE_ATTRIBUTE_DATA as *mut ffi::c_void;
    let result = unsafe { GetFileAttributesExA(file_name, GetFileExInfoStandard, data_ptr) };
    if result == FALSE {
        panic!("GetFileAttributesExA failed");
    }
    data.ftLastWriteTime
}

fn win32_load_game_code(source_dll_name: PCSTR, temp_dll_name: PCSTR) -> Win32GameCode {
    let mut game_code = Win32GameCode {
        game_code_dll: HMODULE::default(),
        last_write_time: FILETIME::default(),
        update_and_render: None,
        get_sound_samples: None,
        is_valid: false,
    };

    unsafe {
        // NOTE(aalhendi): we are using a temp file here because we want to load the DLL into memory,
        // without locking the file which Windows will do if we try to load it directly.
        // TODO(aalhendi): fallible

        game_code.last_write_time = win32_get_last_write_time(source_dll_name);
        CopyFileA(source_dll_name, temp_dll_name, FALSE);
        let game_code_dll_handle = LoadLibraryA(temp_dll_name);

        if !game_code_dll_handle.is_null() {
            game_code.game_code_dll = game_code_dll_handle;

            let update_proc = GetProcAddress(game_code_dll_handle, s!("game_update_and_render"));
            let sound_proc = GetProcAddress(game_code_dll_handle, s!("game_get_sound_samples"));

            game_code.update_and_render =
                mem::transmute::<FARPROC, Option<GameUpdateAndRenderFn>>(update_proc);
            game_code.get_sound_samples =
                mem::transmute::<FARPROC, Option<GameGetSoundSamplesFn>>(sound_proc);
            game_code.is_valid = true;
        }
    }

    game_code
}

fn win32_unload_game_code(game_code: &mut Win32GameCode) {
    if game_code.game_code_dll != HMODULE::default() {
        unsafe {
            // TODO(aalhendi): fallible
            FreeLibrary(game_code.game_code_dll);
        }
        game_code.is_valid = false;
        game_code.get_sound_samples = None;
        game_code.update_and_render = None;
    }
}

#[inline(always)]
fn win32_get_wall_clock() -> i64 {
    let mut perf_count = 0;
    unsafe {
        // TODO(aalhendi): handle error. this is fallible
        QueryPerformanceCounter(&mut perf_count);
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
) -> Result<
    (
        dsound::LPDIRECTSOUND,
        dsound::LPDIRECTSOUNDBUFFER,
        dsound::LPDIRECTSOUNDBUFFER,
    ),
    &'static str,
> {
    use dsound::*;

    unsafe {
        let dsound_lib = LoadLibraryA(s!("dsound.dll"));
        if dsound_lib == 0 as HMODULE {
            return Err("Failed to load dsound.dll");
        }

        let direct_sound_create: DirectSoundCreateFn =
            match GetProcAddress(dsound_lib, s!("DirectSoundCreate")) {
                Some(ds_create_proc) => mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    DirectSoundCreateFn,
                >(ds_create_proc),
                None => return Err("Failed to get DirectSoundCreate address"),
            };

        let mut ds: LPDIRECTSOUND = ptr::null_mut();
        if !SUCCEEDED(direct_sound_create(ptr::null(), &mut ds, ptr::null_mut())) {
            return Err("DirectSoundCreate failed");
        }

        if !SUCCEEDED((*ds).SetCooperativeLevel(window_handle, DSSCL_PRIORITY)) {
            return Err("SetCooperativeLevel failed");
        }

        let primary_buffer_desc = DSBUFFERDESC {
            dwSize: size_of::<DSBUFFERDESC>() as u32,
            dwFlags: DSBCAPS_PRIMARYBUFFER,
            dwBufferBytes: 0,
            dwReserved: 0,
            // NOTE(aalhendi): we actually can't set the format here, we have to set it later via SetFormat. Windows!
            lpwfxFormat: ptr::null_mut(),
            guid3DAlgorithm: GUID::default(),
        };

        let mut primary_buffer: LPDIRECTSOUNDBUFFER = ptr::null_mut();
        if !SUCCEEDED((*ds).CreateSoundBuffer(&primary_buffer_desc, &mut primary_buffer)) {
            return Err("CreateSoundBuffer (Primary) failed");
        }

        let mut wave_format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: 2,
            nBlockAlign: 4, // (nChannels * 16) /8
            nSamplesPerSec: sound_output.samples_per_second,
            nAvgBytesPerSec: sound_output.samples_per_second * 4, // 4 is nBlockAlign // TODO(aalhendi): remove hardcoded 16, use sound_output.bytes_per_sample?
            wBitsPerSample: 16, // TODO(aalhendi): remove hardcoded 16, use sound_output.bytes_per_sample?
            cbSize: 0,
        };

        if !SUCCEEDED((*primary_buffer).SetFormat(&wave_format)) {
            return Err("SetFormat (Primary) failed");
        }

        let secondary_buffer_desc = DSBUFFERDESC {
            dwSize: size_of::<DSBUFFERDESC>() as u32,
            dwFlags: DSBCAPS_GETCURRENTPOSITION2 | DSBCAPS_GLOBALFOCUS | DSBCAPS_CTRL3D,
            dwBufferBytes: sound_output.buffer_size,
            dwReserved: 0,
            lpwfxFormat: &mut wave_format,
            guid3DAlgorithm: GUID::default(),
        };

        let mut secondary_buffer: LPDIRECTSOUNDBUFFER = ptr::null_mut();
        if !SUCCEEDED((*ds).CreateSoundBuffer(&secondary_buffer_desc, &mut secondary_buffer)) {
            return Err("CreateSoundBuffer (Secondary) failed");
        }

        Ok((ds, primary_buffer, secondary_buffer))
    }
}

#[derive(Default, Copy, Clone)]
#[cfg(feature = "internal_build")]
struct Win32DebugTimeMarker {
    pub output_play_cursor: u32,
    pub output_write_cursor: u32,
    pub output_location: u32,
    pub output_byte_count: u32,
    pub expected_flip_play_cursor: u32,

    pub flip_play_cursor: u32,
    pub flip_write_cursor: u32,
}

struct Win32SoundOutput {
    samples_per_second: u32,
    running_sample_index: u32,
    bytes_per_sample: u32,
    buffer_size: u32,
    safety_bytes: u32,
    // TODO(aalhendi): adding a "bytes_per_second" would simplify math.
    // TODO(aalhendi): should runnning sample index be in bytes as well?
}

impl Win32SoundOutput {
    fn new(samples_per_second: u32) -> Self {
        let bytes_per_sample = size_of::<i16>() as u32 * 2;

        Self {
            samples_per_second,
            running_sample_index: 0,
            bytes_per_sample,
            buffer_size: samples_per_second * bytes_per_sample, // 2 channels, 2 bytes per sample
            // TODO(aalhendi): actually compute this variance and see lowest reasonable value
            safety_bytes: ((samples_per_second * bytes_per_sample) / GAME_UPDATE_HZ as u32) / 3,
        }
    }
}

fn win32_clear_sound_buffer(
    secondary_buffer: dsound::LPDIRECTSOUNDBUFFER,
    sound_output: &mut Win32SoundOutput,
) {
    // To be filled by Lock:
    let mut region1_ptr: *mut ffi::c_void = ptr::null_mut();
    let mut region1_size: u32 = 0;
    let mut region2_ptr: *mut ffi::c_void = ptr::null_mut();
    let mut region2_size: u32 = 0;
    unsafe {
        if dsound::SUCCEEDED((*secondary_buffer).Lock(
            0,
            sound_output.buffer_size,
            &mut region1_ptr,
            &mut region1_size,
            &mut region2_ptr,
            &mut region2_size,
            0,
        )) {
            ptr::write_bytes(region1_ptr as *mut u8, 0, region1_size as usize);
            // NOTE(aalhendi): region2 is not null if we are wrapping around the ring buffer... and we expect a contiguous region at startup so this will rarely be hit.
            if !region2_ptr.is_null() {
                ptr::write_bytes(region2_ptr as *mut u8, 0, region2_size as usize);
            }

            (*secondary_buffer).Unlock(region1_ptr, region1_size, region2_ptr, region2_size); // TODO(aalhendi): can fail?
        } else {
            panic!("Secondary buffer Lock failed");
        }
    }
}

fn win32_fill_sound_buffer(
    // note(aalhendi): this is meant to be a "global" secondary buffer, but we are passing it in as an argument for now
    secondary_buffer: dsound::LPDIRECTSOUNDBUFFER,
    sound_output: &mut Win32SoundOutput,
    bytes_to_lock: u32,
    bytes_to_write: u32,
    source_buffer: &mut GameSoundOutputBuffer,
) {
    // To be filled by Lock:
    let mut region1_ptr: *mut ffi::c_void = ptr::null_mut();
    let mut region1_size: u32 = 0;
    let mut region2_ptr: *mut ffi::c_void = ptr::null_mut();
    let mut region2_size: u32 = 0;

    unsafe {
        if dsound::SUCCEEDED((*secondary_buffer).Lock(
            bytes_to_lock,
            bytes_to_write,
            &mut region1_ptr,
            &mut region1_size,
            &mut region2_ptr,
            &mut region2_size,
            0,
        )) {
            // TODO(aalhendi): assert that region1_size , region2_size are valid
            let region1_sample_count = region1_size / sound_output.bytes_per_sample;
            let mut dest_sample = region1_ptr as *mut i16;
            let mut source_sample = source_buffer.samples;

            for _ in 0..region1_sample_count {
                // basically, we write L/R L/R L/R L/R etc.
                // we use sample_out as an i16 ptr to the memory location we want to write to (region1 / ringbuffer)
                *dest_sample = *source_sample;
                dest_sample = dest_sample.add(1);
                source_sample = source_sample.add(1);
                *dest_sample = *source_sample;
                dest_sample = dest_sample.add(1);
                source_sample = source_sample.add(1);
                sound_output.running_sample_index =
                    sound_output.running_sample_index.wrapping_add(1);
            }

            // in the case where we are wrapping around the ring buffer, we need to fill region2
            // todo(aalhendi): same loop as above, but for region2, can we collapse the 2 loops?
            if !region2_ptr.is_null() {
                let region2_sample_count = region2_size / sound_output.bytes_per_sample;
                dest_sample = region2_ptr as *mut i16;
                for _ in 0..region2_sample_count {
                    *dest_sample = *source_sample;
                    dest_sample = dest_sample.add(1);
                    source_sample = source_sample.add(1);
                    *dest_sample = *source_sample;
                    dest_sample = dest_sample.add(1);
                    source_sample = source_sample.add(1);
                    sound_output.running_sample_index =
                        sound_output.running_sample_index.wrapping_add(1);
                }
            }

            (*secondary_buffer).Unlock(region1_ptr, region1_size, region2_ptr, region2_size);
        } else {
            panic!("Secondary buffer Lock failed");
        }
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
    //  CreateDIBSection also can't haveThe fn can only have one signature, it can't get an u8ptr OR an u64 ptr etc. so we pass a void* and cast appropriately
    //  it is used as a double ptr because we give windows an addr of a ptr which we want it to OVERWRITE into a NEW PTR which would point to where it alloc'd mem
    memory: *mut ffi::c_void,
    // NOTE(aalhendi): We store width and height in self.info.bmiHeader. This is redundant. Keeping because its only 8 bytes
    width: i32,
    height: i32,
    pitch: i32,
    bytes_per_pixel: i32,
}

impl Win32OffscreenBuffer {
    unsafe fn win32_copy_buffer_to_window(&self, device_context: HDC, _width: i32, _height: i32) {
        // NOTE(aalhendi): for prototyping purposes, we're going to always blit 1-1 pixels to make sure we don't introduce artifacts
        //  with stretching until we get a decent renderer
        unsafe {
            StretchDIBits(
                device_context,
                0,
                0,
                self.width,
                self.height,
                0,
                0,
                self.width,
                self.height,
                self.memory,
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
            // TODO(aalhendi): check result
            if free_res == FALSE {
                panic!("Failed to free memory");
            }
        }

        self.width = width;
        self.height = height;

        let bitmap_info_header = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: self.width,
            // NOTE(aalhendi): When bHeight is negative, it clues Windows to treat the bitmap as top-down rather than bottom-up. This means that the first three bytes are for the top-left pixel.
            biHeight: -self.height,
            biPlanes: 1,
            biBitCount: 32, // 8 for red, 8 for green, 8 for blue, ask for 32 for DWORD alignment
            biCompression: BI_RGB, // Uncompressed
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };

        self.info.bmiHeader = bitmap_info_header;

        let bitmap_memory_size = (self.bytes_per_pixel * self.width * self.height) as usize;
        self.memory = unsafe {
            VirtualAlloc(
                ptr::null(),
                bitmap_memory_size,
                MEM_RESERVE | MEM_COMMIT,
                PAGE_READWRITE,
            )
        };

        self.pitch = width * self.bytes_per_pixel;
        // TODO(aalhendi): Probably clear this to black
    }

    #[cfg(feature = "internal_build")]
    fn debug_sync_display(
        &mut self,
        markers: &mut [Win32DebugTimeMarker],
        current_marker_idx: usize,
        sound_output: &mut Win32SoundOutput,
        _seconds_per_frame: f64,
    ) {
        let pad_x = 16; // pixels
        let pad_y = 16; // pixels

        let line_height = 64;

        let coefficient = (self.width - (2 * pad_x)) as f32 / sound_output.buffer_size as f32;

        // NOTE(aalhendi): calculates the x coordinate of a given point in the sound buffer
        let c_x = |pt: u32| pad_x + (pt as f32 * coefficient) as i32;

        for (idx, marker) in markers.iter().enumerate() {
            let mut top = pad_y;
            let mut bottom = pad_y + line_height;

            assert!(marker.flip_play_cursor < sound_output.buffer_size);
            assert!(marker.flip_write_cursor < sound_output.buffer_size);
            assert!(marker.output_play_cursor < sound_output.buffer_size);
            assert!(marker.output_write_cursor < sound_output.buffer_size);
            assert!(marker.output_location < sound_output.buffer_size);
            assert!(marker.output_byte_count < sound_output.buffer_size); // clipped anyway, kinda useless
            // Expected flip cursor is an added value, so it's allowed to be greater than the buffer size.

            if idx == current_marker_idx {
                top += pad_y + line_height;
                bottom += pad_y + line_height;
                let first_top = top;

                self.debug_draw_vertical(c_x(marker.output_play_cursor), top, bottom, 0xFFFFFFFF);
                self.debug_draw_vertical(c_x(marker.output_write_cursor), top, bottom, 0xFFFF0000);

                top += pad_y + line_height;
                bottom += pad_y + line_height;

                self.debug_draw_vertical(c_x(marker.output_location), top, bottom, 0xFFFFFFFF);
                self.debug_draw_vertical(
                    c_x(marker.output_location + marker.output_byte_count),
                    top,
                    bottom,
                    0xFFFF0000,
                );

                top += pad_y + line_height;
                bottom += pad_y + line_height;

                self.debug_draw_vertical(
                    c_x(marker.expected_flip_play_cursor),
                    first_top,
                    bottom,
                    0xFFFFFF00,
                );
            };

            self.debug_draw_vertical(c_x(marker.flip_play_cursor), top, bottom, 0xFFFFFFFF);
            self.debug_draw_vertical(
                c_x(marker.flip_play_cursor + (480 * sound_output.bytes_per_sample)),
                top,
                bottom,
                0xFFFF00FF,
            );
            self.debug_draw_vertical(c_x(marker.flip_write_cursor), top, bottom, 0xFFFF0000);
        }
    }

    #[cfg(feature = "internal_build")]
    fn debug_draw_vertical(&self, x: i32, top: i32, bottom: i32, color: u32) {
        let top = if top <= 0 { 0 } else { top };

        let bottom = if bottom > self.height {
            self.height
        } else {
            bottom
        };

        if x < 0 || x >= self.width {
            return;
        }

        let mut pixel = unsafe {
            self.memory
                .cast::<u8>()
                .add((x * self.bytes_per_pixel) as usize)
                .offset((top * self.pitch) as isize)
        };
        for _y in top..bottom {
            unsafe {
                *(pixel as *mut u32) = color;
                pixel = pixel.add(self.pitch as usize);
            };
        }
    }
}

fn win32_begin_recording_input(state: &mut Win32State, input_recording_idx: u32) {
    state.input_recording_idx = input_recording_idx;
    // TODO(aalhendi): These files must fo in a temp/build directory!
    // TODO(aalhendi): lazily write giant memory block and use a memory copy instead?

    let filename = PCSTR::from(c"foo.hmi".as_ptr().cast::<u8>());
    state.recording_file_handle = unsafe {
        CreateFileA(
            filename,
            GENERIC_WRITE,
            FILE_SHARE_NONE,
            ptr::null(),
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    if state.recording_file_handle == INVALID_HANDLE_VALUE {
        panic!("Failed to create recording file");
    }

    let bytes_to_write = state.total_size as u32;
    debug_assert_eq!(state.total_size, bytes_to_write as usize);
    let mut bytes_written = 0_u32;
    let write_result = unsafe {
        WriteFile(
            state.recording_file_handle,
            state.game_memory_block.cast::<u8>(),
            bytes_to_write,
            &mut bytes_written,
            ptr::null_mut(),
        )
    };
    if write_result == FALSE || bytes_written != bytes_to_write {
        eprintln!("Failed to write recording file: {write_result:?}");
    }
}

fn win32_end_recording_input(state: &mut Win32State) {
    unsafe {
        CloseHandle(state.recording_file_handle);
    }
    state.input_recording_idx = 0;
}

fn win32_begin_input_playback(state: &mut Win32State, input_playing_idx: u32) {
    state.input_playing_idx = input_playing_idx;

    let filename = PCSTR::from(c"foo.hmi".as_ptr().cast::<u8>());
    state.playback_file_handle = unsafe {
        CreateFileA(
            filename,
            GENERIC_READ,
            FILE_SHARE_READ,
            ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            0 as HANDLE,
        )
    };

    let bytes_to_read = state.total_size as u32;
    debug_assert_eq!(state.total_size, bytes_to_read as usize);
    let mut bytes_read = 0_u32;
    let read_result = unsafe {
        ReadFile(
            state.playback_file_handle,
            state.game_memory_block.cast::<u8>(),
            bytes_to_read,
            &mut bytes_read,
            ptr::null_mut(),
        )
    };
    if read_result == FALSE || bytes_read != bytes_to_read {
        eprintln!("Failed to read recording file: {read_result:?}");
    }
}

fn win32_end_input_playback(state: &mut Win32State) {
    unsafe {
        CloseHandle(state.playback_file_handle);
    }
    state.input_playing_idx = 0;
}

fn win32_record_input(state: &mut Win32State, new_input: &mut GameInput) {
    let memory_size = size_of::<GameInput>() as u32;
    let mut bytes_written = 0_u32;
    let write_result = unsafe {
        WriteFile(
            state.recording_file_handle,
            (new_input as *mut GameInput).cast::<u8>(),
            memory_size,
            &mut bytes_written,
            ptr::null_mut(),
        )
    };
    if write_result == FALSE || bytes_written != memory_size {
        eprintln!("Failed to write into recording file: {write_result:?}");
    }
}

fn win32_playback_input(state: &mut Win32State, new_input: &mut GameInput) {
    let mut bytes_read = 0_u32;
    let file_size = size_of::<GameInput>() as u32;
    let read_result = unsafe {
        ReadFile(
            state.playback_file_handle,
            (new_input as *mut GameInput).cast::<u8>(),
            file_size,
            &mut bytes_read,
            ptr::null_mut(),
        )
    };
    if read_result == TRUE && bytes_read == 0 {
        // NOTE(aalhendi): we've hit the end of the stream, go back to the beginning
        let playing_idx = state.input_playing_idx;
        win32_end_input_playback(state);
        win32_begin_input_playback(state, playing_idx);
        unsafe {
            ReadFile(
                state.playback_file_handle,
                (new_input as *mut GameInput).cast::<u8>(),
                file_size,
                &mut bytes_read,
                ptr::null_mut(),
            );
        }
    }
}

fn main() {
    unsafe {
        // TODO(aalhendi): fallible
        let module_handle = GetModuleHandleA(ptr::null());
        let mut state = Win32State::default();

        // TODO(aalhendi): fallible
        QueryPerformanceFrequency(&mut PERF_COUNT_FREQUENCY);

        state.win32_get_exe_file_name(module_handle);

        let mut source_dll_name = Win32Path([0; MAX_PATH_USIZE]);
        let mut temp_dll_name = Win32Path([0; MAX_PATH_USIZE]);
        state.win32_build_exe_path_file_name(c"hm.dll", &mut source_dll_name);
        state.win32_build_exe_path_file_name(c"game_temp.dll", &mut temp_dll_name);

        // NOTE(aalhendi): Set windows scheduler granularity. This is used to make our sleep more accurate (granular).
        let desired_scheduler_ms = 1;
        let sleep_is_granular = timeBeginPeriod(desired_scheduler_ms) == TIMERR_NOERROR;
        if !sleep_is_granular {
            println!("Sleep is not granular. This is bad.");
        }

        GLOBAL_BACKBUFFER.win32_resize_dib_section(1280, 720);

        // TODO(aalhendi): getmodulehandlea check result fallible
        let instance = HINSTANCE::from(GetModuleHandleA(ptr::null()));

        let wc = WNDCLASSA {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(win32_main_window_callback),
            hInstance: instance,
            lpszClassName: s!("HandmadeHeroWindowClass"),
            hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW), // fallible
            ..Default::default()
        };

        let atom = RegisterClassA(&wc);
        if atom == 0 {
            // TODO(aalhendi): Logging
            // return
        }
        debug_assert!(atom != 0);

        // TODO(aalhendi): fallible
        let window_handle = CreateWindowExA(
            WS_EX_TOPMOST | WS_EX_LAYERED,
            wc.lpszClassName,
            s!("Handmade Hero"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            ptr::null_mut(),
            ptr::null_mut(),
            HINSTANCE::from(module_handle),
            ptr::null(),
        );

        // NOTE(aalhendi): Audio test
        // TODO(aalhendi): make this sixty seconds?
        let mut sound_output = Win32SoundOutput::new(48000);

        // can't load direct sound till we have a window handle
        let (_ds, _primary_buffer, secondary_buffer) =
            match win32_init_dsound(window_handle, &mut sound_output) {
                Ok(ds_and_primary_buffer) => ds_and_primary_buffer,
                Err(e) => {
                    eprintln!("Failed to initialize DirectSound: {e}");
                    return;
                }
            };

        win32_clear_sound_buffer(secondary_buffer, &mut sound_output);
        (*secondary_buffer).Play(0, 0, dsound::DSBPLAY_LOOPING); // TODO(aalhendi): fallible

        GLOBAL_RUNNING = true;

        /*
        // NOTE(aalhendi): this tests the PlayCursor/WriteCursor update frequency
        // On my machine, it was 480 samples.
        loop {
            let mut play_cursor = 0_u32;
            let mut write_cursor = 0_u32;
            let _ = secondary_buffer
                .GetCurrentPosition(Some(&mut play_cursor), Some(&mut write_cursor));
            println!("PC:{play_cursor} WC: {write_cursor}");
        }
         */

        // TODO(aalhendi): pool with bitmap VirtualAlloc
        let samples = VirtualAlloc(
            ptr::null(),
            sound_output.buffer_size as usize,
            MEM_RESERVE | MEM_COMMIT,
            PAGE_READWRITE,
        ) as *mut i16;

        let permanent_storage_size = megabytes_to_bytes(64);
        let transient_storage_size = gigabytes_to_bytes(1);
        // TODO(aalhendi): handle various memory footprints (USING SYSTEM METRICS)
        let total_storage_size = permanent_storage_size + transient_storage_size;
        let base_address = {
            #[cfg(feature = "internal_build")]
            {
                use interface::terabytes_to_bytes;
                terabytes_to_bytes(2)
            }
            #[cfg(not(feature = "internal_build"))]
            {
                0_usize
            }
        };

        state.total_size = total_storage_size;
        // TODO(aalhendi): look into using MEM_LARGE_PAGES?
        state.game_memory_block = VirtualAlloc(
            base_address as *mut ffi::c_void,
            state.total_size,
            MEM_RESERVE | MEM_COMMIT,
            PAGE_READWRITE,
        ) as *mut ();
        let permanent_storage = state.game_memory_block;

        let mut game_memory = GameMemory {
            is_initialized: false,
            permanent_storage_size,
            transient_storage_size,
            permanent_storage,
            transient_storage: permanent_storage
                .cast::<u8>()
                .add(permanent_storage_size)
                .cast::<()>(),
            #[cfg(feature = "internal_build")]
            debug_platform_read_entire_file,
            #[cfg(feature = "internal_build")]
            debug_platform_write_entire_file,
            #[cfg(feature = "internal_build")]
            debug_platform_free_file_memory,
        };

        if samples.is_null()
            || game_memory.permanent_storage.is_null()
            || game_memory.transient_storage.is_null()
        {
            panic!("Failed to allocate samples or permanent storage");
        }

        let mut input = [GameInput::default(), GameInput::default()];
        // NOTE(aalhendi): this is a hack to get around the fact that we can't have 2 mutable references to the same array
        let (new_input_slice, old_input_slice) = input.split_at_mut(1);

        let mut new_input = &mut new_input_slice[0];
        let mut old_input = &mut old_input_slice[0];

        let mut last_counter = win32_get_wall_clock();
        let mut flip_wall_clock = win32_get_wall_clock();

        #[cfg(feature = "internal_build")]
        let mut debug_time_marker_idx = 0;
        #[cfg(feature = "internal_build")]
        let mut debug_time_markers = [Win32DebugTimeMarker::default(); GAME_UPDATE_HZ as usize / 2];

        #[cfg(feature = "internal_build")]
        let mut audio_latency_bytes;
        #[cfg(feature = "internal_build")]
        let mut audio_latency_sec;
        let mut is_sound_valid = false;

        let mut game = win32_load_game_code(source_dll_name.as_ptr(), temp_dll_name.as_ptr());

        // TODO(aalhendi): do we want to use rdtscp instead?
        let mut last_cycle_count = x86_64::_rdtsc();

        while GLOBAL_RUNNING {
            let new_dll_write_time = win32_get_last_write_time(source_dll_name.as_ptr());
            if CompareFileTime(&new_dll_write_time, &game.last_write_time) != 0 {
                win32_unload_game_code(&mut game);
                game = win32_load_game_code(source_dll_name.as_ptr(), temp_dll_name.as_ptr());
                game.last_write_time = new_dll_write_time;
            }

            let new_keyboard_controller = &mut new_input.controllers[0];
            let old_keyboard_controller = &mut old_input.controllers[0];
            // TODO(aalhendi): we can't zero everything because the up/down count will be wrong
            *new_keyboard_controller = GameControllerInput::default();
            new_keyboard_controller.is_connected = true;
            for (i, button) in new_keyboard_controller.buttons.iter_mut().enumerate() {
                button.ended_down = old_keyboard_controller.buttons[i].ended_down;
            }

            win32_process_pending_messages(&mut state, new_keyboard_controller);

            if !GLOBAL_PAUSE {
                // TODO(aalhendi): should we poll this more frequently?
                // TODO(aalhendi): need to not poll disconnected controllers to avoid xinput frame hit on older libraries
                let mut max_controller_count = XUSER_MAX_COUNT as usize;
                if max_controller_count > new_input.controllers.len() {
                    max_controller_count = new_input.controllers.len();
                }
                // NOTE(aalhendi): max controller count minus the keyboard controller
                for controller_index in 0..max_controller_count - 1 {
                    let old_controller = &mut old_input.controllers[controller_index];
                    let new_controller = &mut new_input.controllers[controller_index];

                    let mut controller_state: XINPUT_STATE = XINPUT_STATE::default();
                    let x_input_state_res =
                        XInputGetState(controller_index as u32, &mut controller_state);
                    if x_input_state_res == ERROR_SUCCESS {
                        new_controller.is_connected = true;
                        new_controller.is_analog = old_controller.is_analog;

                        // NOTE(aalhendi): This controller is connected
                        // TODO(aalhendi): see if controller_state.dwPacketNumber increments too rapidly
                        let pad = &controller_state.Gamepad;

                        let stick_left_x = win32_process_x_input_stick_value(
                            pad.sThumbLX,
                            XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE as i16,
                        );
                        new_controller.left_stick_average_x = stick_left_x;

                        let stick_left_y = win32_process_x_input_stick_value(
                            pad.sThumbLY,
                            XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE as i16,
                        );
                        new_controller.left_stick_average_y = stick_left_y;

                        if stick_left_x != 0.0 || stick_left_y != 0.0 {
                            new_controller.is_analog = true;
                        }

                        // TODO(aalhendi): add right stick support

                        if (pad.wButtons & XINPUT_GAMEPAD_DPAD_UP) != 0 {
                            new_controller.is_analog = false;
                            new_controller.left_stick_average_y = 1.0;
                        }
                        if (pad.wButtons & XINPUT_GAMEPAD_DPAD_DOWN) != 0 {
                            new_controller.is_analog = false;
                            new_controller.left_stick_average_y = -1.0;
                        }
                        if (pad.wButtons & XINPUT_GAMEPAD_DPAD_LEFT) != 0 {
                            new_controller.is_analog = false;
                            new_controller.left_stick_average_x = 1.0;
                        }
                        if (pad.wButtons & XINPUT_GAMEPAD_DPAD_RIGHT) != 0 {
                            new_controller.is_analog = false;
                            new_controller.left_stick_average_x = -1.0;
                        }

                        let threshold = 0.5_f32;
                        // NOTE(aalhendi): fake dpad emulation from left stick
                        win32_process_x_input_digital_button(
                            XINPUT_GAMEPAD_BUTTON_FLAGS::from(
                                (new_controller.left_stick_average_x < -threshold) as u16,
                            ),
                            old_controller.button_mut(GameButton::MoveLeft),
                            XINPUT_GAMEPAD_DPAD_LEFT,
                            new_controller.button_mut(GameButton::MoveLeft),
                        );
                        win32_process_x_input_digital_button(
                            XINPUT_GAMEPAD_BUTTON_FLAGS::from(
                                (new_controller.left_stick_average_x > threshold) as u16,
                            ),
                            old_controller.button_mut(GameButton::MoveRight),
                            XINPUT_GAMEPAD_DPAD_RIGHT,
                            new_controller.button_mut(GameButton::MoveRight),
                        );
                        win32_process_x_input_digital_button(
                            XINPUT_GAMEPAD_BUTTON_FLAGS::from(
                                (new_controller.left_stick_average_y < -threshold) as u16,
                            ),
                            old_controller.button_mut(GameButton::MoveUp),
                            XINPUT_GAMEPAD_DPAD_UP,
                            new_controller.button_mut(GameButton::MoveUp),
                        );
                        win32_process_x_input_digital_button(
                            XINPUT_GAMEPAD_BUTTON_FLAGS::from(
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
                let vibration = XINPUT_VIBRATION {
                    wLeftMotorSpeed: 65535,
                    wRightMotorSpeed: 65535,
                };
                XInputSetState(0, &vibration);

                let mut buffer = GameOffscreenBuffer {
                    width: GLOBAL_BACKBUFFER.width,
                    height: GLOBAL_BACKBUFFER.height,
                    pitch: GLOBAL_BACKBUFFER.pitch,
                    bytes_per_pixel: GLOBAL_BACKBUFFER.bytes_per_pixel,
                    memory: GLOBAL_BACKBUFFER.memory,
                };
                if state.input_recording_idx == 1 {
                    win32_record_input(&mut state, new_input);
                }
                if state.input_playing_idx == 1 {
                    win32_playback_input(&mut state, new_input);
                }

                if let Some(update_and_render) = game.update_and_render {
                    update_and_render(&mut game_memory, new_input, &mut buffer);
                }

                /*
                NOTE(aalhendi): Here is how sound output computation works.

                We define a safety value that is the number of samples we think our game update loop
                may vary by (let's say up to 2 ms)

                When we wake up to write audio, we will look and see what the play cursor position is,
                and we will forecast ahead where we think the play cursor will be on the next frame boundary.

                We will then look to see if the write cursor is before that by our safe amount.
                If it is, the target fill position is that frame boundary plus one frame.
                This gives us perfect audio sync in the case of a card that has low enough latency.

                If the write cursor is *after* that safety margin,
                then we assume we can never sync the audio perfectly,
                 so we will write one frame's worth of audio plus the safety margin's worth of guard samples.
                 */
                let audio_wall_clock = win32_get_wall_clock();
                let from_begin_to_audio_seconds =
                    win32_get_seconds_elapsed(flip_wall_clock, audio_wall_clock);
                let mut play_cursor = 0_u32;
                let mut write_cursor = 0_u32;

                if dsound::SUCCEEDED(
                    (*secondary_buffer).GetCurrentPosition(&mut play_cursor, &mut write_cursor),
                ) {
                    if !is_sound_valid {
                        sound_output.running_sample_index =
                            write_cursor / sound_output.bytes_per_sample;
                        is_sound_valid = true;
                    }

                    // lock offset
                    let bytes_to_lock = sound_output
                        .running_sample_index
                        .overflowing_mul(sound_output.bytes_per_sample)
                        .0
                        % sound_output.buffer_size;

                    let expected_sound_bytes_per_frame = (sound_output.samples_per_second
                        * sound_output.bytes_per_sample)
                        / GAME_UPDATE_HZ as u32;
                    let seconds_left_until_flip =
                        TARGET_SECONDS_PER_FRAME - from_begin_to_audio_seconds;
                    let expected_bytes_until_flip =
                        ((seconds_left_until_flip / TARGET_SECONDS_PER_FRAME)
                            * expected_sound_bytes_per_frame as f64) as u32;
                    let expected_frame_boundary_byte = play_cursor + expected_bytes_until_flip;
                    let safe_write_cursor = if write_cursor < play_cursor {
                        write_cursor + sound_output.buffer_size + sound_output.safety_bytes
                    } else {
                        write_cursor + sound_output.safety_bytes
                    };
                    debug_assert!(safe_write_cursor - sound_output.safety_bytes >= play_cursor);
                    let is_low_latency_audio_card =
                        safe_write_cursor < expected_frame_boundary_byte;

                    // lock size
                    let target_write_cursor = if is_low_latency_audio_card {
                        (expected_frame_boundary_byte + expected_sound_bytes_per_frame)
                            % sound_output.buffer_size
                    } else {
                        (write_cursor + expected_sound_bytes_per_frame + sound_output.safety_bytes)
                            % sound_output.buffer_size
                    };
                    let bytes_to_write = if bytes_to_lock > target_write_cursor {
                        // case 1: we are wrapping around the ring buffer, fill 2 regions
                        (sound_output.buffer_size - bytes_to_lock) + target_write_cursor
                    } else {
                        // case 2: we are not wrapping around the ring buffer, fill 1 region
                        target_write_cursor - bytes_to_lock
                    };

                    #[cfg(feature = "internal_build")]
                    {
                        let current_debug_marker = &mut debug_time_markers[debug_time_marker_idx];

                        current_debug_marker.output_play_cursor = play_cursor;
                        current_debug_marker.output_write_cursor = write_cursor;
                        current_debug_marker.output_location = bytes_to_lock;
                        current_debug_marker.output_byte_count = bytes_to_write;
                        current_debug_marker.expected_flip_play_cursor =
                            expected_frame_boundary_byte;

                        let unwrapped_write_cursor = if write_cursor < play_cursor {
                            write_cursor + sound_output.buffer_size
                        } else {
                            write_cursor
                        };

                        audio_latency_bytes = unwrapped_write_cursor - play_cursor;
                        audio_latency_sec = (audio_latency_bytes as f32
                            / sound_output.bytes_per_sample as f32)
                            / sound_output.samples_per_second as f32;

                        println!(
                            "BTL:{bytes_to_lock} TC:{target_write_cursor} BTW:{bytes_to_write} - PC:{play_cursor} WC:{write_cursor} DELTA:{audio_latency_bytes} ({audio_latency_sec:.3}s)"
                        );
                    }

                    let mut sound_buffer = GameSoundOutputBuffer {
                        samples_per_second: sound_output.samples_per_second,
                        sample_count: bytes_to_write / sound_output.bytes_per_sample,
                        samples,
                    };

                    if let Some(get_sound_samples) = game.get_sound_samples {
                        get_sound_samples(&mut game_memory, &mut sound_buffer);
                    }

                    // NOTE(aalhendi): ideally, we want to only fill sound buffer if there's something to write.
                    // this fn calls Lock(), which can fail if bytes_to_write is 0,
                    // we are ignoring the error for now, skipping this frame if it fails. This is to match C behavior.
                    // from testing, this only happens the first time the call occurs. we could have an if check to see if bytes_to_write is 0,
                    // but that check would run every frame, which is not ideal.
                    win32_fill_sound_buffer(
                        secondary_buffer,
                        &mut sound_output,
                        bytes_to_lock,
                        bytes_to_write,
                        &mut sound_buffer,
                    );
                } else {
                    is_sound_valid = false;
                }

                // TODO(aalhendi): NOT TESTED YET!
                let work_counter = win32_get_wall_clock();
                let work_seconds_elapsed = win32_get_seconds_elapsed(last_counter, work_counter);

                let mut seconds_elapsed_for_frame = work_seconds_elapsed;
                if seconds_elapsed_for_frame < TARGET_SECONDS_PER_FRAME {
                    while seconds_elapsed_for_frame < TARGET_SECONDS_PER_FRAME {
                        if sleep_is_granular {
                            let sleep_ms =
                                (TARGET_SECONDS_PER_FRAME - seconds_elapsed_for_frame) * 1000_f64;
                            if sleep_ms > 1_f64 {
                                // TODO(aalhendi): sleeping is hard... see Intel's TPAUSE instruction. I think AMD uses UMWAIT.
                                windows_sys::Win32::System::Threading::Sleep(sleep_ms as u32);
                            }
                        }

                        #[cfg(feature = "internal_build")]
                        {
                            let test_seconds_elapsed_for_frame =
                                win32_get_seconds_elapsed(last_counter, win32_get_wall_clock());
                            const FRAME_TIME_SLOP_S: f64 = 0.002;
                            debug_assert!(
                                test_seconds_elapsed_for_frame
                                    < TARGET_SECONDS_PER_FRAME + FRAME_TIME_SLOP_S,
                                "Test seconds elapsed for frame is greater than target seconds per frame {test_seconds_elapsed_for_frame} > {TARGET_SECONDS_PER_FRAME}"
                            );
                        }

                        seconds_elapsed_for_frame =
                            win32_get_seconds_elapsed(last_counter, win32_get_wall_clock());
                    }
                } else {
                    // TODO(aalhendi): handle missed frame
                    println!(
                        "MISSED TARGET FPS!!! {seconds_elapsed_for_frame} < {TARGET_SECONDS_PER_FRAME}"
                    );
                }

                let end_counter = win32_get_wall_clock();
                let ms_per_frame = 1_000_f64 * win32_get_seconds_elapsed(last_counter, end_counter);
                last_counter = end_counter;

                let dims = Win32WindowDimension::from(window_handle);
                #[cfg(feature = "internal_build")]
                GLOBAL_BACKBUFFER.debug_sync_display(
                    &mut debug_time_markers,
                    if debug_time_marker_idx == 0 {
                        debug_time_marker_idx
                    } else {
                        debug_time_marker_idx - 1
                    },
                    &mut sound_output,
                    TARGET_SECONDS_PER_FRAME,
                );

                let device_context = GetDC(window_handle);
                GLOBAL_BACKBUFFER.win32_copy_buffer_to_window(
                    device_context,
                    dims.width,
                    dims.height,
                );
                ReleaseDC(window_handle, device_context);

                flip_wall_clock = win32_get_wall_clock();

                #[cfg(feature = "internal_build")]
                {
                    // NOTE(aalhendi): This is debug code
                    let mut play_cursor = 0_u32;
                    let mut write_cursor = 0_u32;
                    if !dsound::SUCCEEDED(
                        (*secondary_buffer).GetCurrentPosition(&mut play_cursor, &mut write_cursor),
                    ) {
                        panic!("Failed to get current position from secondary buffer");
                    }
                    debug_assert!(debug_time_marker_idx <= debug_time_markers.len());
                    let current_debug_marker = &mut debug_time_markers[debug_time_marker_idx];

                    current_debug_marker.flip_play_cursor = play_cursor;
                    current_debug_marker.flip_write_cursor = write_cursor;

                    debug_time_marker_idx += 1;
                    if debug_time_marker_idx == debug_time_markers.len() {
                        debug_time_marker_idx = 0;
                    }
                }

                mem::swap(&mut new_input, &mut old_input);
                // TODO(aalhendi): should i clear these here?

                let end_cycle_count = x86_64::_rdtsc();
                let cycles_elapsed = end_cycle_count as f64 - last_cycle_count as f64;
                last_cycle_count = end_cycle_count;

                let fps = 0_f64; // TODO(aalhendi): calculate fps
                println!(
                    "{ms_per_frame:.2} ms/frame - {fps:.1} fps - {mc:.2} mega_cycles/frame",
                    mc = cycles_elapsed / (1_000_f64 * 1_000_f64)
                );
            }
        }
    }
}

unsafe fn win32_process_pending_messages(
    state: &mut Win32State,
    keyboard_controller: &mut GameControllerInput,
) {
    let mut message = MSG::default();
    while unsafe { PeekMessageA(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) != FALSE } {
        match message.message {
            WM_QUIT => unsafe {
                GLOBAL_RUNNING = false;
            },
            WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP => {
                let virtual_key_code = message.wParam;
                let was_down = (message.lParam & (1 << KEY_MESSAGE_WAS_DOWN_BIT)) != 0;
                let is_down = (message.lParam & (1 << KEY_MESSAGE_IS_DOWN_BIT)) == 0;
                let is_alt_down = (message.lParam & (1 << KEY_MESSAGE_IS_ALT_BIT)) != 0;
                if was_down != is_down {
                    match VIRTUAL_KEY::from(virtual_key_code as u16) {
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
                        VK_P => {
                            #[cfg(feature = "internal_build")]
                            unsafe {
                                if is_down {
                                    GLOBAL_PAUSE = !GLOBAL_PAUSE
                                }
                            }
                        }
                        VK_L => {
                            #[cfg(feature = "internal_build")]
                            {
                                if is_down {
                                    if state.input_recording_idx == 0 {
                                        win32_begin_recording_input(state, 1);
                                    } else {
                                        win32_end_recording_input(state);
                                        win32_begin_input_playback(state, 1);
                                    }
                                }
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
                LRESULT::from(0_isize)
            }
            WM_DESTROY => {
                // TODO(aalhendi): Handle this as an error - recreate window?
                GLOBAL_RUNNING = false;
                LRESULT::from(0_isize)
            }
            WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP => {
                debug_assert!(
                    false,
                    "Keyboard input came in through a non-dispatch event. This should not happen. Means it likely came through via callback."
                );
                LRESULT::from(0_isize)
            }

            WM_ACTIVATEAPP => {
                println!("WM_ACTIVATE");
                let b_alpha = if wparam == TRUE as usize { 255 } else { 64 };
                SetLayeredWindowAttributes(window, rgb(0, 0, 0), b_alpha, LWA_ALPHA);
                LRESULT::from(0_isize)
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
                LRESULT::from(0_isize)
            }
            _ => DefWindowProcA(window, message, wparam, lparam),
        }
    }
}

#[cfg(feature = "internal_build")]
/// # Safety
/// TODO(aalhendi): idk. write this up
pub unsafe extern "C" fn debug_platform_free_file_memory(ptr: *mut ffi::c_void) {
    unsafe {
        if !ptr.is_null() {
            // TODO(aalhendi): hanlde the result
            let _ = VirtualFree(ptr, 0, MEM_RELEASE);
        } else {
            eprintln!("Failed to free file memory: ptr is null");
        }
    }
}

#[cfg(feature = "internal_build")]
pub extern "C" fn debug_platform_write_entire_file(
    filename: *const ffi::c_char,
    memory_size: u32,
    memory: *mut ffi::c_void,
) -> bool {
    unsafe {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, GENERIC_WRITE},
            Storage::FileSystem::{
                CREATE_ALWAYS, CreateFileA, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE, WriteFile,
            },
        };

        let filename = PCSTR::from(filename.cast::<u8>());
        let file_handle = CreateFileA(
            filename,
            GENERIC_WRITE,
            FILE_SHARE_NONE,
            ptr::null(),
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        );
        if file_handle == INVALID_HANDLE_VALUE {
            eprintln!("Failed to create file");
            return false;
        }

        let mut bytes_written = 0_u32;
        let write_result = WriteFile(
            file_handle,
            memory.cast::<u8>(),
            memory_size,
            &mut bytes_written,
            ptr::null_mut(),
        );
        if write_result == FALSE || bytes_written != memory_size {
            eprintln!("Failed to write file: {write_result:?}");
        }

        // NOTE(aalhendi): we COULD have a RAII guard for the file handle, but it's not worth the complexity.
        // where we impl Drop for FileHandleGuard, and we closethe handle in the drop.
        // so for now we just close it manually.
        // TODO(aalhendi): handle the result
        let _ = CloseHandle(file_handle);

        true
    }
}

#[cfg(feature = "internal_build")]
pub extern "C" fn debug_platform_read_entire_file(
    filename: *const ffi::c_char,
) -> DebugPlatformReadFileResult {
    unsafe {
        use interface::safe_truncate_i64_to_u32;
        use windows_sys::Win32::{
            Foundation::{CloseHandle, GENERIC_READ, GetLastError},
            Storage::FileSystem::{
                CreateFileA, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, GetFileSizeEx, OPEN_EXISTING,
                ReadFile,
            },
        };

        let filename = PCSTR::from(filename.cast::<u8>());
        let file_handle = CreateFileA(
            filename,
            GENERIC_READ,
            FILE_SHARE_READ,
            ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            0 as HANDLE,
        ); // TODO(aalhendi): fallible

        let mut file_size = 0_i64;
        GetFileSizeEx(file_handle, &mut file_size); // TODO(aalhendi): fallible
        let file_size_u32 = safe_truncate_i64_to_u32(file_size);

        if file_size == 0 {
            // NOTE(aalhendi): Reading an empty file isn't an error. We can return null or allocate a 1 byte buffer if caller expects a non-null ptr.
            // we will return null and let the caller handle it.
            return DebugPlatformReadFileResult {
                memory: ptr::null_mut(),
                size: 0,
            };
        }

        let mut memory_ptr =
            VirtualAlloc(ptr::null(), file_size as usize, MEM_COMMIT, PAGE_READWRITE);

        if memory_ptr.is_null() {
            panic!(
                "Failed to allocate memory for file. Error Code: {}",
                GetLastError()
            );
        }

        let mut bytes_read = 0_u32;
        let read_result = ReadFile(
            file_handle,
            memory_ptr.cast::<u8>(),
            file_size_u32,
            &mut bytes_read,
            ptr::null_mut(),
        );
        // TODO(aalhendi): check result
        if read_result == FALSE || bytes_read != file_size_u32 {
            debug_platform_free_file_memory(memory_ptr);
            eprintln!("Failed to read file: {read_result:?}");
            // sound because we just freed the memory
            memory_ptr = ptr::null_mut();
        }

        // NOTE(aalhendi): we COULD have a RAII guard for the file handle, but it's not worth the complexity.
        // where we impl Drop for FileHandleGuard, and we closethe handle in the drop.
        // so for now we just close it manually.
        // TODO(aalhendi): handle the result
        let _ = CloseHandle(file_handle);
        DebugPlatformReadFileResult {
            memory: memory_ptr,
            size: file_size_u32,
        }
    }
}

// NOTE(aalhendi): `windows-sys` in Rust does NOT support COM interfaces like DirectSound.
// The alternative `windows` crate does support COM interfaces, but it's fat.
#[allow(non_snake_case)]
pub mod dsound {
    use core::ffi::c_void;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Media::Audio::WAVEFORMATEX;
    use windows_sys::core::GUID;

    #[repr(C)]
    pub struct DSBUFFERDESC {
        pub dwSize: u32,
        pub dwFlags: u32,
        pub dwBufferBytes: u32,
        pub dwReserved: u32,
        pub lpwfxFormat: *mut WAVEFORMATEX,
        pub guid3DAlgorithm: GUID,
    }

    pub const DSSCL_PRIORITY: u32 = 2u32;

    pub const DSBCAPS_PRIMARYBUFFER: u32 = 1u32;

    pub const DSBCAPS_GETCURRENTPOSITION2: u32 = 65536u32;

    pub const DSBCAPS_GLOBALFOCUS: u32 = 32768u32;

    pub const DSBCAPS_CTRL3D: u32 = 16u32;

    pub const DSBPLAY_LOOPING: u32 = 1u32;

    pub type HRESULT = i32;
    pub type LPDIRECTSOUND = *mut IDirectSound;
    pub type LPDIRECTSOUNDBUFFER = *mut IDirectSoundBuffer;

    pub type DirectSoundCreateFn = unsafe extern "system" fn(
        pcGuidDevice: *const GUID,
        ppDS: *mut LPDIRECTSOUND,
        pUnkOuter: *mut c_void,
    ) -> HRESULT;

    // Helper to check HRESULT
    #[inline(always)]
    pub fn SUCCEEDED(hr: HRESULT) -> bool {
        hr >= 0
    }

    #[repr(C)]
    pub struct IDirectSound {
        pub lpVtbl: *const IDirectSoundVtbl,
    }

    #[repr(C)]
    pub struct IDirectSoundVtbl {
        pub QueryInterface: usize,
        pub AddRef: usize,
        pub Release: unsafe extern "system" fn(this: LPDIRECTSOUND) -> u32,
        pub CreateSoundBuffer: unsafe extern "system" fn(
            this: LPDIRECTSOUND,
            pcDSBufferDesc: *const DSBUFFERDESC,
            ppDSBuffer: *mut LPDIRECTSOUNDBUFFER,
            pUnkOuter: *mut c_void,
        ) -> HRESULT,
        pub GetCaps: usize,
        pub DuplicateSoundBuffer: usize,
        pub SetCooperativeLevel:
            unsafe extern "system" fn(this: LPDIRECTSOUND, hwnd: HWND, dwLevel: u32) -> HRESULT,
    }

    // Ergonomic wrappers so we don't have to write pointer math in our main loop
    impl IDirectSound {
        /// # Safety
        /// unsafe. windows
        #[inline]
        pub unsafe fn SetCooperativeLevel(&mut self, hwnd: HWND, level: u32) -> HRESULT {
            unsafe { ((*self.lpVtbl).SetCooperativeLevel)(self, hwnd, level) }
        }

        /// # Safety
        /// unsafe. windows
        #[inline]
        pub unsafe fn CreateSoundBuffer(
            &mut self,
            desc: *const DSBUFFERDESC,
            buffer: *mut LPDIRECTSOUNDBUFFER,
        ) -> HRESULT {
            unsafe { ((*self.lpVtbl).CreateSoundBuffer)(self, desc, buffer, core::ptr::null_mut()) }
        }
    }

    #[repr(C)]
    pub struct IDirectSoundBuffer {
        pub lpVtbl: *const IDirectSoundBufferVtbl,
    }

    #[repr(C)]
    pub struct IDirectSoundBufferVtbl {
        pub QueryInterface: usize,
        pub AddRef: usize,
        pub Release: unsafe extern "system" fn(this: LPDIRECTSOUNDBUFFER) -> u32,
        pub GetCaps: usize,
        pub GetCurrentPosition: unsafe extern "system" fn(
            this: LPDIRECTSOUNDBUFFER,
            pdwCurrentPlayCursor: *mut u32,
            pdwCurrentWriteCursor: *mut u32,
        ) -> HRESULT,
        pub GetFormat: usize,
        pub GetVolume: usize,
        pub GetPan: usize,
        pub GetFrequency: usize,
        pub GetStatus: usize,
        pub Initialize: usize,
        pub Lock: unsafe extern "system" fn(
            this: LPDIRECTSOUNDBUFFER,
            dwOffset: u32,
            dwBytes: u32,
            ppvAudioPtr1: *mut *mut c_void,
            pdwAudioBytes1: *mut u32,
            ppvAudioPtr2: *mut *mut c_void,
            pdwAudioBytes2: *mut u32,
            dwFlags: u32,
        ) -> HRESULT,
        pub Play: unsafe extern "system" fn(
            this: LPDIRECTSOUNDBUFFER,
            dwReserved1: u32,
            dwPriority: u32,
            dwFlags: u32,
        ) -> HRESULT,
        pub SetCurrentPosition: usize,
        pub SetFormat: unsafe extern "system" fn(
            this: LPDIRECTSOUNDBUFFER,
            pcfxFormat: *const WAVEFORMATEX,
        ) -> HRESULT,
        pub SetVolume: usize,
        pub SetPan: usize,
        pub SetFrequency: usize,
        pub Stop: usize,
        pub Unlock: unsafe extern "system" fn(
            this: LPDIRECTSOUNDBUFFER,
            pvAudioPtr1: *const c_void,
            dwAudioBytes1: u32,
            pvAudioPtr2: *const c_void,
            dwAudioBytes2: u32,
        ) -> HRESULT,
    }

    impl IDirectSoundBuffer {
        /// # Safety
        /// unsafe. windows
        #[inline]
        pub unsafe fn SetFormat(&mut self, format: *const WAVEFORMATEX) -> HRESULT {
            unsafe { ((*self.lpVtbl).SetFormat)(self, format) }
        }

        /// # Safety
        /// unsafe. windows
        #[inline]
        pub unsafe fn Play(&mut self, reserved1: u32, priority: u32, flags: u32) -> HRESULT {
            unsafe { ((*self.lpVtbl).Play)(self, reserved1, priority, flags) }
        }

        /// # Safety
        /// unsafe. windows
        #[inline]
        pub unsafe fn GetCurrentPosition(
            &mut self,
            play_cursor: *mut u32,
            write_cursor: *mut u32,
        ) -> HRESULT {
            unsafe { ((*self.lpVtbl).GetCurrentPosition)(self, play_cursor, write_cursor) }
        }

        /// # Safety
        /// unsafe. windows
        #[allow(clippy::too_many_arguments)]
        #[inline]
        pub unsafe fn Lock(
            &mut self,
            offset: u32,
            bytes: u32,
            ptr1: *mut *mut c_void,
            bytes1: *mut u32,
            ptr2: *mut *mut c_void,
            bytes2: *mut u32,
            flags: u32,
        ) -> HRESULT {
            unsafe { ((*self.lpVtbl).Lock)(self, offset, bytes, ptr1, bytes1, ptr2, bytes2, flags) }
        }

        /// # Safety
        /// unsafe. windows
        #[inline]
        pub unsafe fn Unlock(
            &mut self,
            ptr1: *const c_void,
            bytes1: u32,
            ptr2: *const c_void,
            bytes2: u32,
        ) -> HRESULT {
            unsafe { ((*self.lpVtbl).Unlock)(self, ptr1, bytes1, ptr2, bytes2) }
        }
    }
}
