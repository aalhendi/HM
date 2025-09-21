// TODO(aalhendi): Services that the platform layer provides to the game

// NOTE(aalhendi): Services that the game provides to the platform layer

pub struct GameOffscreenBuffer {
    // NOTE(aalhendi): pixels are always 32-bits wide, Memory Order BB GG RR XX
    // NOTE(aalhendi): void* to avoid specifying the type, we want windows to give us back a ptr to the bitmap memory
    //  windows doesn't know (on the API lvl), what sort of flags, and therefore what kind of memory we want.
    //  CreateDIBSection also can't haveThe fn can only have one signature, it cant get a u8ptr OR a u64 ptr etc. so we pass a void* and cast appropriately
    //  it is used as a double ptr because we give windows an addr of a ptr which we want it to OVERWRITE into a NEW PTR which would point to where it alloc'd mem
    pub memory: *mut std::ffi::c_void,
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
    pub controllers: [GameControllerInput; 4],
}

#[derive(Default)]
pub struct GameButtonState {
    pub half_transition_count: u32,
    pub ended_down: bool,
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
                T_SINE += 2_f32 * std::f32::consts::PI * 1_f32 / wave_period as f32;
            }
        }
    }
}

pub fn game_update_and_render(
    input: &mut GameInput,
    buffer: &mut GameOffscreenBuffer,
    sound_buffer: &mut GameSoundOutputBuffer,
) {
    unsafe {
        static mut BLUE_OFFSET: i32 = 0;
        static mut GREEN_OFFSET: i32 = 0;
        static mut TONE_HZ: u32 = 256; // Hz

        let input_0 = &mut input.controllers[0];

        if input_0.is_analog {
            // NOTE(aalhendi): use analog tuning
            BLUE_OFFSET += (4.0_f32 * input_0.left_stick_x_end) as i32;
            TONE_HZ = 256 + (128_f32 * input_0.left_stick_y_end) as u32;
        } else {
            // NOTE(aalhendi): use digital tuning
        }

        if input_0.button(GameButton::Down).ended_down {
            GREEN_OFFSET += 1;
        }

        // TODO(aalhendi): allow sample offsets here for more robust platform options
        sound_buffer.game_output_sound(TONE_HZ);
        buffer.render_weird_gradient(BLUE_OFFSET, GREEN_OFFSET);
    }
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
