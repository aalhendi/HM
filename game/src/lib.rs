// NOTE(aalhendi): Services that the platform layer provides to the game

// NOTE(aalhendi): Services that the game provides to the platform layer

use core::f32;
use interface::{GameInput, GameMemory, GameOffscreenBuffer, GameSoundOutputBuffer, ThreadContext};

#[derive(Default)]
#[repr(C)]
pub struct GameState {}

fn game_output_sound(
    _thread: &mut ThreadContext,
    game_state: &mut GameState,
    buffer: &mut GameSoundOutputBuffer,
    tone_hz: u32,
) {
    let tone_volume = 3000;
    let wave_period = buffer.samples_per_second / tone_hz;

    unsafe {
        let mut sample_out = buffer.samples;
        for _sample_index in 0..buffer.sample_count {
            // let sine_value = f32::sin(game_state.t_sine);
            // let sample_value = (sine_value * tone_volume as f32) as i16;
            let sample_value = 0;

            // basically, we write L/R L/R L/R L/R etc.
            // we use sample_out as an i16 ptr to the memory location we want to write to (region1 / ringbuffer)
            *sample_out = sample_value;
            sample_out = sample_out.offset(1);
            *sample_out = sample_value;
            sample_out = sample_out.offset(1);
            // // move 1 sample worth forward
            // game_state.t_sine += 2_f32 * f32::consts::PI * 1_f32 / wave_period as f32;
            // if game_state.t_sine > 2_f32 * f32::consts::PI {
            //     game_state.t_sine -= 2_f32 * f32::consts::PI;
            // }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn game_update_and_render(
    thread: &mut ThreadContext,
    memory: &mut GameMemory,
    input: &mut GameInput,
    buffer: &mut GameOffscreenBuffer,
) {
    debug_assert!(
        size_of::<GameState>() <= memory.permanent_storage_size,
        "GameState is too large for permanent storage"
    );

    let game_state = unsafe { &mut *memory.permanent_storage.cast::<GameState>() };

    if !memory.is_initialized {
        // TODO(aalhendi): this may be more appropriate in to do in the platform layer
        memory.is_initialized = true;
    }

    for controller in input.controllers.iter_mut() {
        if controller.is_analog {
            // NOTE(aalhendi): use analog tuning
        } else {
            // NOTE(aalhendi): use digital tuning
        }
    }

    draw_rectangle(buffer, 10.0, 10.0, 30.0, 30.0);
}

// NOTE(aalhendi): At the moment, this has to be a very fast function, it cannot be more than a millisecond
// or so.
// TODO(aalhendi): reduce the pressure on this function's performance by measuring it or asking about it.
#[unsafe(no_mangle)]
pub extern "C" fn game_get_sound_samples(
    thread: &mut ThreadContext,
    memory: &mut GameMemory,
    sound_buffer: &mut GameSoundOutputBuffer,
) {
    let game_state = unsafe { &mut *memory.permanent_storage.cast::<GameState>() };
    game_output_sound(thread, game_state, sound_buffer, 400);
}

fn draw_rectangle(
    buffer: &mut GameOffscreenBuffer,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
) {
    let mut int_min_x = min_x.round() as i32;
    let mut int_min_y = min_y.round() as i32;
    let mut int_max_x = max_x.round() as i32;
    let mut int_max_y = max_y.round() as i32;

    // clipping rules
    if int_min_x < 0 {
        int_min_x = 0;
    }
    if int_min_y < 0 {
        int_min_y = 0;
    }
    if int_max_x >= buffer.width {
        int_max_x = buffer.width;
    }
    if int_max_y >= buffer.height {
        int_max_y = buffer.height;
    }

    let color = 0xFFFFFFFF;
    let mut row = unsafe {
        buffer
            .memory
            .cast::<u8>()
            .add((int_min_x * buffer.bytes_per_pixel) as usize)
            .offset((int_min_y * buffer.pitch) as isize)
    };
    for y in int_min_y..int_max_y {
        let mut pixel = row.cast::<u32>();
        for _x in int_min_x..int_max_x {
            unsafe {
                *pixel = color;
                pixel = pixel.add(1);
            };
        }
        row = unsafe { row.add(buffer.pitch as usize) };
    }
}
