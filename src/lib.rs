// NOTE(aalhendi): Services that the platform layer provides to the game

#[cfg(feature = "internal_build")]
/// # Safety
/// TODO(aalhendi): idk. write this up
pub unsafe fn debug_platform_free_file_memory(ptr: *mut core::ffi::c_void) {
    unsafe {
        use windows::Win32::System::Memory::{MEM_RELEASE, VirtualFree};
        if !ptr.is_null() {
            // TODO(aalhendi): hanlde the result
            let _ = VirtualFree(ptr, 0, MEM_RELEASE);
        } else {
            eprintln!("Failed to free file memory: ptr is null");
        }
    }
}

#[cfg(feature = "internal_build")]
pub fn debug_platform_write_entire_file(
    filename: PCSTR,
    memory: &[u8],
) -> windows::core::Result<()> {
    unsafe {
        use windows::Win32::{
            Foundation::{CloseHandle, GENERIC_WRITE},
            Storage::FileSystem::{
                CREATE_ALWAYS, CreateFileA, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE, WriteFile,
            },
        };

        let file_handle = CreateFileA(
            filename,
            GENERIC_WRITE.0,
            FILE_SHARE_NONE,
            None,
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )?;

        let mut bytes_written = 0_u32;
        let write_result = WriteFile(file_handle, Some(memory), Some(&mut bytes_written), None);
        if write_result.is_err() || bytes_written as usize != memory.len() {
            eprintln!("Failed to write file: {write_result:?}");
        }

        // NOTE(aalhendi): we COULD have a RAII guard for the file handle, but it's not worth the complexity.
        // where we impl Drop for FileHandleGuard, and we closethe handle in the drop.
        // so for now we just close it manually.
        // TODO(aalhendi): handle the result
        let _ = CloseHandle(file_handle);
        Ok(())
    }
}

#[cfg(feature = "internal_build")]
pub struct DebugPlatformReadFileResult {
    pub memory: *mut core::ffi::c_void,
    // NOTE(aalhendi): we are limited to u32 because of the Windows API... Its debug anyway.
    pub size: u32,
}

#[cfg(feature = "internal_build")]
pub fn debug_platform_read_entire_file(
    filename: PCSTR,
) -> windows::core::Result<DebugPlatformReadFileResult> {
    unsafe {
        use windows::Win32::{
            Foundation::{CloseHandle, GENERIC_READ},
            Storage::FileSystem::{
                CreateFileA, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, GetFileSizeEx, OPEN_EXISTING,
                ReadFile,
            },
            System::Memory::{MEM_COMMIT, PAGE_READWRITE, VirtualAlloc},
        };

        let file_handle = CreateFileA(
            filename,
            GENERIC_READ.0,
            FILE_SHARE_READ,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )?;

        let mut file_size = 0_i64;
        GetFileSizeEx(file_handle, &mut file_size)?;
        let file_size_u32 = safe_truncate_i64_to_u32(file_size);

        if file_size == 0 {
            // NOTE(aalhendi): Reading an empty file isn't an error. We can return null or allocate a 1 byte buffer if caller expects a non-null ptr.
            // we will return null and let caller handle it.
            return Ok(DebugPlatformReadFileResult {
                memory: std::ptr::null_mut(),
                size: 0,
            });
        }

        let mut memory_ptr = VirtualAlloc(None, file_size as usize, MEM_COMMIT, PAGE_READWRITE);

        if memory_ptr.is_null() {
            eprintln!("Failed to allocate memory for file");
            return Err(windows::core::Error::from_win32());
        }

        let buffer = std::slice::from_raw_parts_mut(memory_ptr as *mut u8, file_size_u32 as usize);

        let mut bytes_read = 0_u32;
        let read_result = ReadFile(file_handle, Some(buffer), Some(&mut bytes_read), None);
        if read_result.is_err() || bytes_read != file_size_u32 {
            debug_platform_free_file_memory(memory_ptr);
            eprintln!("Failed to read file: {read_result:?}");
            // sound because we just freed the memory
            memory_ptr = std::ptr::null_mut();
        }

        // NOTE(aalhendi): we COULD have a RAII guard for the file handle, but it's not worth the complexity.
        // where we impl Drop for FileHandleGuard, and we closethe handle in the drop.
        // so for now we just close it manually.
        // TODO(aalhendi): handle the result
        let _ = CloseHandle(file_handle);
        Ok(DebugPlatformReadFileResult {
            memory: memory_ptr,
            size: file_size_u32,
        })
    }
}

// NOTE(aalhendi): Services that the game provides to the platform layer

use core::{f32, mem};

#[cfg(feature = "internal_build")]
use windows::core::PCSTR;

/// Converts megabytes to bytes.
#[inline(always)]
pub const fn megabytes_to_bytes(megabytes: usize) -> usize {
    megabytes * 1024 * 1024
}

/// Converts gigabytes to bytes.
#[inline(always)]
pub const fn gigabytes_to_bytes(gigabytes: usize) -> usize {
    gigabytes * 1024 * 1024 * 1024
}

#[cfg(feature = "internal_build")]
/// Converts terabytes to bytes.
#[inline(always)]
pub const fn terabytes_to_bytes(terabytes: usize) -> usize {
    terabytes * 1024 * 1024 * 1024 * 1024
}

#[cfg(feature = "internal_build")]
#[inline(always)]
pub fn safe_truncate_i64_to_u32(value: i64) -> u32 {
    debug_assert!(value < u32::MAX as i64, "Value is too large");
    value as u32
}

pub struct GameOffscreenBuffer {
    // NOTE(aalhendi): pixels are always 32-bits wide, Memory Order BB GG RR XX
    // NOTE(aalhendi): void* to avoid specifying the type, we want windows to give us back a ptr to the bitmap memory
    //  windows doesn't know (on the API lvl), what sort of flags, and therefore what kind of memory we want.
    //  CreateDIBSection also can't haveThe fn can only have one signature, it cant get a u8ptr OR a u64 ptr etc. so we pass a void* and cast appropriately
    //  it is used as a double ptr because we give windows an addr of a ptr which we want it to OVERWRITE into a NEW PTR which would point to where it alloc'd mem
    pub memory: *mut core::ffi::c_void,
    // NOTE(aalhendi): We store width and height in self.info.bmiHeader. This is redundant. Keeping because its only 8 bytes
    pub width: i32,
    pub height: i32,
    pub pitch: isize,
}

pub struct GameSoundOutputBuffer {
    pub samples_per_second: u32,
    pub sample_count: u32,
    pub samples: *mut i16,
}

#[repr(u8)]
pub enum GameButton {
    Up = 0,
    Down,
    Left,
    Right,
    RightShoulder,
    LeftShoulder,
}

#[derive(Default)]
pub struct GameControllerInput {
    pub is_analog: bool,

    pub left_stick_x_start: f32,
    pub left_stick_y_start: f32,

    pub left_stick_x_min: f32,
    pub left_stick_y_min: f32,

    pub left_stick_x_max: f32,
    pub left_stick_y_max: f32,

    pub left_stick_x_end: f32,
    pub left_stick_y_end: f32,

    pub buttons: [GameButtonState; 6],
}

impl GameControllerInput {
    /// A helper method that abstracts away the indexing and casting.
    #[inline(always)]
    pub fn button(&self, button: GameButton) -> &GameButtonState {
        &self.buttons[button as usize]
    }

    /// A helper method that abstracts away the indexing and casting.
    #[inline(always)]
    pub fn button_mut(&mut self, button: GameButton) -> &mut GameButtonState {
        &mut self.buttons[button as usize]
    }
}

#[derive(Default)]
pub struct GameInput {
    // TODO(aalhendi): insert clock values here.
    pub controllers: [GameControllerInput; 4],
}

#[derive(Default)]
pub struct GameButtonState {
    pub half_transition_count: u32,
    pub ended_down: bool,
}

#[derive(Default)]
pub struct GameMemory {
    pub is_initialized: bool,
    pub permanent_storage_size: usize,
    // NOTE(aalhendi): REQUIRED to be cleared to 0 at startup
    pub permanent_storage: *mut (),
    pub transient_storage_size: usize,
    // NOTE(aalhendi): REQUIRED to be cleared to 0 at startup
    pub transient_storage: *mut (),
}

#[derive(Default)]
pub struct GameState {
    pub tone_hz: u32,
    pub blue_offset: i32,
    pub green_offset: i32,
}

impl GameSoundOutputBuffer {
    pub fn game_output_sound(&mut self, tone_hz: u32) {
        let tone_volume = 3000;
        let wave_period = self.samples_per_second / tone_hz;

        // NOTE(aalhendi): CURRENTLY USING: static mut (direct C equivalent)
        // direct mem access, no runtime overhead, needs unsafe for every access.
        // no thread safety. if called from multiple threads, will have UB.
        // ALTERNATIVES:
        // Cell<T> (near identical perf, safe interior mut, not thread safe but mem safe. compiles to same asm.)
        // OnceLock<Cell<T>>
        // Mutex<T>
        // AtomicT (thread safe, min perf overhead)
        unsafe {
            static mut T_SINE: f32 = 0.0;

            let mut sample_out = self.samples;
            for _sample_index in 0..self.sample_count {
                let sine_value = f32::sin(T_SINE);
                let sample_value = (sine_value * tone_volume as f32) as i16;

                // basically, we write L/R L/R L/R L/R etc.
                // we use sample_out as an i16 ptr to the memory location we want to write to (region1 / ringbuffer)
                *sample_out = sample_value;
                sample_out = sample_out.offset(1);
                *sample_out = sample_value;
                sample_out = sample_out.offset(1);
                // move 1 sample worth forward
                T_SINE += 2_f32 * f32::consts::PI * 1_f32 / wave_period as f32;
            }
        }
    }
}

pub fn game_update_and_render(
    memory: &mut GameMemory,
    input: &mut GameInput,
    buffer: &mut GameOffscreenBuffer,
    sound_buffer: &mut GameSoundOutputBuffer,
) {
    debug_assert!(
        mem::size_of::<GameState>() <= memory.permanent_storage_size,
        "GameState is too large for permanent storage"
    );

    let game_state = unsafe { &mut *memory.permanent_storage.cast::<GameState>() };

    if !memory.is_initialized {
        #[cfg(feature = "internal_build")]
        {
            let filename = windows::core::s!("src/main.rs");
            let read_result =
                debug_platform_read_entire_file(filename).expect("Failed to read file");

            let memory = unsafe {
                std::slice::from_raw_parts_mut(
                    read_result.memory as *mut u8,
                    read_result.size as usize,
                )
            };
            debug_platform_write_entire_file(windows::core::s!("test.out"), memory)
                .expect("Failed to write file");

            unsafe {
                debug_platform_free_file_memory(read_result.memory);
            }
        }

        game_state.tone_hz = 256;
        // NOTE(aalhendi): these are not needed because they are cleared to 0 at startup by requirement!
        // game_state.blue_offset = 0;
        // game_state.green_offset = 0;

        // TODO(aalhendi): this may be more appropriate in to do in the platform layer
        memory.is_initialized = true;
    }

    let input_0 = &mut input.controllers[0];

    if input_0.is_analog {
        // NOTE(aalhendi): use analog tuning
        game_state.blue_offset += (4.0_f32 * input_0.left_stick_x_end) as i32;
        game_state.tone_hz = 256 + (128_f32 * input_0.left_stick_y_end) as u32;
    } else {
        // NOTE(aalhendi): use digital tuning
    }

    if input_0.button(GameButton::Down).ended_down {
        game_state.green_offset += 1;
    }

    // TODO(aalhendi): allow sample offsets here for more robust platform options
    sound_buffer.game_output_sound(game_state.tone_hz);
    buffer.render_weird_gradient(game_state.blue_offset, game_state.green_offset);
}

impl GameOffscreenBuffer {
    pub fn render_weird_gradient(&self, blue_offset: i32, green_offset: i32) {
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
}
