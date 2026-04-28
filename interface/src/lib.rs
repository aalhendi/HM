#[repr(C)]
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
    pub pitch: i32,
    pub bytes_per_pixel: i32,
}

#[repr(C)]
pub struct GameSoundOutputBuffer {
    pub samples_per_second: u32,
    pub sample_count: u32,
    pub samples: *mut i16,
}

#[repr(C)]
pub struct GameMemory {
    pub is_initialized: bool,
    pub permanent_storage_size: usize,
    // NOTE(aalhendi): REQUIRED to be cleared to 0 at startup
    pub permanent_storage: *mut (),
    pub transient_storage_size: usize,
    // NOTE(aalhendi): REQUIRED to be cleared to 0 at startup
    pub transient_storage: *mut (),

    // NOTE(aalhendi): The platform layer will fill these in
    #[cfg(feature = "internal_build")]
    pub debug_platform_read_entire_file: DebugPlatformReadEntireFileFn,
    #[cfg(feature = "internal_build")]
    pub debug_platform_write_entire_file: DebugPlatformWriteEntireFileFn,
    #[cfg(feature = "internal_build")]
    pub debug_platform_free_file_memory: DebugPlatformFreeFileMemoryFn,
}

impl Default for GameMemory {
    fn default() -> Self {
        #[cfg(feature = "internal_build")]
        unsafe extern "C" fn default_read_file(
            _thread_context: &mut ThreadContext,
            _filename: *const core::ffi::c_char,
        ) -> DebugPlatformReadFileResult {
            unimplemented!("default_read_file is not implemented for this platform")
        }

        #[cfg(feature = "internal_build")]
        unsafe extern "C" fn default_write_file(
            _thread_context: &mut ThreadContext,
            _filename: *const core::ffi::c_char,
            _memory_size: u32,
            _memory: *mut core::ffi::c_void,
        ) -> bool {
            unimplemented!("default_write_file is not implemented for this platform")
        }

        #[cfg(feature = "internal_build")]
        unsafe extern "C" fn default_free_memory(
            _thread_context: &mut ThreadContext,
            _memory: *mut core::ffi::c_void,
        ) {
            unimplemented!("default_free_memory is not implemented for this platform")
        }

        Self {
            is_initialized: false,
            permanent_storage_size: 0,
            permanent_storage: core::ptr::null_mut(),
            transient_storage_size: 0,
            transient_storage: core::ptr::null_mut(),
            #[cfg(feature = "internal_build")]
            debug_platform_read_entire_file: default_read_file,
            #[cfg(feature = "internal_build")]
            debug_platform_write_entire_file: default_write_file,
            #[cfg(feature = "internal_build")]
            debug_platform_free_file_memory: default_free_memory,
        }
    }
}

pub type GameUpdateAndRenderFn = unsafe extern "C" fn(
    thread: &mut ThreadContext,
    memory: &mut GameMemory,
    input: &mut GameInput,
    buffer: &mut GameOffscreenBuffer,
);

pub type GameGetSoundSamplesFn = unsafe extern "C" fn(
    thread: &mut ThreadContext,
    memory: &mut GameMemory,
    sound_buffer: &mut GameSoundOutputBuffer,
);

#[cfg(feature = "internal_build")]
#[repr(C)]
pub struct DebugPlatformReadFileResult {
    pub memory: *mut core::ffi::c_void,
    // NOTE(aalhendi): we are limited to u32 because of the Windows API... Its debug anyway.
    pub size: u32,
}

#[cfg(feature = "internal_build")]
pub type DebugPlatformReadEntireFileFn = unsafe extern "C" fn(
    thread_context: &mut ThreadContext,
    filename: *const core::ffi::c_char,
) -> DebugPlatformReadFileResult;

#[cfg(feature = "internal_build")]
pub type DebugPlatformWriteEntireFileFn = unsafe extern "C" fn(
    thread_context: &mut ThreadContext,
    filename: *const core::ffi::c_char,
    memory_size: u32,
    memory: *mut core::ffi::c_void,
) -> bool;

#[cfg(feature = "internal_build")]
pub type DebugPlatformFreeFileMemoryFn =
    unsafe extern "C" fn(thread_context: &mut ThreadContext, memory: *mut core::ffi::c_void);

#[repr(C)]
pub enum GameButton {
    MoveUp = 0,
    MoveDown,
    MoveLeft,
    MoveRight,

    ActionUp,
    ActionDown,
    ActionLeft,
    ActionRight,

    RightShoulder,
    LeftShoulder,

    Start,
    Back,
}

#[derive(Default)]
#[repr(C)]
pub struct GameButtonState {
    pub half_transition_count: u32,
    pub ended_down: bool,
}

#[derive(Default)]
#[repr(C)]
pub struct GameControllerInput {
    pub is_connected: bool,
    pub is_analog: bool,

    pub left_stick_average_x: f32,
    pub left_stick_average_y: f32,
    pub right_stick_average_x: f32,
    pub right_stick_average_y: f32,

    // pub buttons: [GameButtonState; core::mem::variant_count::<GameButton>()],
    pub buttons: [GameButtonState; 12], // TODO(aalhendi): mem::variant_count is not stable yet.
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
#[repr(C)]
pub struct GameInput {
    pub mouse_buttons: [GameButtonState; 5],
    pub mouse_x: i32,
    pub mouse_y: i32,
    pub mouse_z: i32,

    pub dt_for_frame: f64,
    pub controllers: [GameControllerInput; 5], // 4 controllers + 1 keyboard
}

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

#[derive(Default)]
#[repr(C)]
pub struct ThreadContext {
    placeholder: u32,
}
