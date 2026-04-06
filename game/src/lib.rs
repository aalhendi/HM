// NOTE(aalhendi): Services that the platform layer provides to the game

// NOTE(aalhendi): Services that the game provides to the platform layer

use core::{f32, mem};
#[cfg(feature = "internal_build")]
use core::{ffi, slice};
use interface::{GameButton, GameInput, GameMemory, GameOffscreenBuffer, GameSoundOutputBuffer};

#[derive(Default)]
#[repr(C)]
pub struct GameState {
    pub tone_hz: u32,
    pub blue_offset: i32,
    pub green_offset: i32,

    pub t_sine: f32,

    pub player_x: i32,
    pub player_y: i32,
    pub t_jump: f32,
}

fn game_output_sound(game_state: &mut GameState, buffer: &mut GameSoundOutputBuffer, tone_hz: u32) {
    let tone_volume = 3000;
    let wave_period = buffer.samples_per_second / tone_hz;

    unsafe {
        let mut sample_out = buffer.samples;
        for _sample_index in 0..buffer.sample_count {
            let sine_value = f32::sin(game_state.t_sine);
            let sample_value = (sine_value * tone_volume as f32) as i16;

            // basically, we write L/R L/R L/R L/R etc.
            // we use sample_out as an i16 ptr to the memory location we want to write to (region1 / ringbuffer)
            *sample_out = sample_value;
            sample_out = sample_out.offset(1);
            *sample_out = sample_value;
            sample_out = sample_out.offset(1);
            // move 1 sample worth forward
            game_state.t_sine += 2_f32 * f32::consts::PI * 1_f32 / wave_period as f32;
            if game_state.t_sine > 2_f32 * f32::consts::PI {
                game_state.t_sine -= 2_f32 * f32::consts::PI;
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn game_update_and_render(
    memory: &mut GameMemory,
    input: &mut GameInput,
    buffer: &mut GameOffscreenBuffer,
) {
    debug_assert!(
        mem::size_of::<GameState>() <= memory.permanent_storage_size,
        "GameState is too large for permanent storage"
    );

    let game_state = unsafe { &mut *memory.permanent_storage.cast::<GameState>() };

    if !memory.is_initialized {
        #[cfg(feature = "internal_build")]
        {
            let filename = c"game/src/lib.rs".as_ptr();
            let read_result = unsafe { (memory.debug_platform_read_entire_file)(filename) };

            let file_memory = unsafe {
                slice::from_raw_parts_mut(read_result.memory as *mut u8, read_result.size as usize)
            };

            unsafe {
                (memory.debug_platform_write_entire_file)(
                    c"test.out".as_ptr(),
                    file_memory.len() as u32,
                    file_memory.as_mut_ptr().cast::<ffi::c_void>(),
                );

                (memory.debug_platform_free_file_memory)(read_result.memory);
            }
        }

        game_state.tone_hz = 512;
        game_state.t_sine = 0_f32;

        game_state.player_x = 100;
        game_state.player_y = 100;

        // NOTE(aalhendi): these are not needed because they are cleared to 0 at startup by requirement!
        // game_state.blue_offset = 0;
        // game_state.green_offset = 0;

        // TODO(aalhendi): this may be more appropriate in to do in the platform layer
        memory.is_initialized = true;
    }

    for controller in input.controllers.iter_mut() {
        if controller.is_analog {
            // NOTE(aalhendi): use analog tuning
            game_state.blue_offset += (4.0_f32 * controller.left_stick_average_x) as i32;
            game_state.tone_hz = 512 + (128_f32 * controller.left_stick_average_y) as u32;
        } else {
            // NOTE(aalhendi): use digital tuning
            if controller.button(GameButton::MoveLeft).ended_down {
                game_state.blue_offset -= 1;
            }
            if controller.button(GameButton::MoveRight).ended_down {
                game_state.blue_offset += 1;
            }
        }

        if controller.button(GameButton::ActionDown).ended_down {
            game_state.green_offset += 1;
        }
        game_state.player_x += (4.0_f32 * controller.left_stick_average_x) as i32;
        game_state.player_y -= (4.0_f32 * controller.left_stick_average_y) as i32;

        if game_state.t_jump > 0.0 {
            game_state.player_y +=
                (5_f32 * f32::sin(0.5 * f32::consts::PI * game_state.t_jump)) as i32
        }
        if controller.button(GameButton::ActionDown).ended_down {
            game_state.t_jump = 4.0;
        }
        game_state.t_jump -= 0.033;
    }

    render_weird_gradient(buffer, game_state.blue_offset, game_state.green_offset);
    render_player(buffer, game_state.player_x, game_state.player_y);
}

// NOTE(aalhendi): At the moment, this has to be a very fast function, it cannot be more than a millisecond
// or so.
// TODO(aalhendi): reduce the pressure on this function's performance by measuring it or asking about it.
#[unsafe(no_mangle)]
pub extern "C" fn game_get_sound_samples(
    memory: &mut GameMemory,
    sound_buffer: &mut GameSoundOutputBuffer,
) {
    let game_state = unsafe { &mut *memory.permanent_storage.cast::<GameState>() };
    game_output_sound(game_state, sound_buffer, game_state.tone_hz);
}

fn render_weird_gradient(buffer: &mut GameOffscreenBuffer, blue_offset: i32, green_offset: i32) {
    let width = buffer.width;
    let height = buffer.height;

    let mut row = buffer.memory as *const u8;
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
        row = unsafe { row.offset(buffer.pitch as isize) };
    }
}

fn render_player(buffer: &mut GameOffscreenBuffer, player_x: i32, player_y: i32) {
    let end_of_buffer = unsafe {
        buffer
            .memory
            .cast::<u8>()
            .add((buffer.pitch * buffer.height) as usize)
    };
    let color = 0xFFFFFFFF;
    let top = player_y;
    let bottom = player_y + 10;

    for x in player_x..player_x + 10 {
        let mut pixel = unsafe {
            buffer
                .memory
                .cast::<u8>()
                .add((x * buffer.bytes_per_pixel) as usize)
                .offset((top * buffer.pitch) as isize)
        };
        for _y in top..bottom {
            unsafe {
                if pixel >= buffer.memory.cast::<u8>() && pixel < end_of_buffer {
                    *(pixel as *mut u32) = color;
                    pixel = pixel.add(buffer.pitch as usize);
                }
            };
        }
    }
}
