// NOTE(aalhendi): Services that the platform layer provides to the game

// NOTE(aalhendi): Services that the game provides to the platform layer

use core::f32;
use interface::GameButton::{MoveDown, MoveLeft, MoveRight, MoveUp};
use interface::{GameInput, GameMemory, GameOffscreenBuffer, GameSoundOutputBuffer, ThreadContext};

#[derive(Default)]
#[repr(C)]
pub struct GameState {
    player_x: f32,
    player_y: f32,
}

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
            let mut d_player_x = 0.0; // pixels/s
            let mut d_player_y = 0.0; // pixels/s
            if controller.button(MoveUp).ended_down {
                d_player_y = -1.0;
            }
            if controller.button(MoveDown).ended_down {
                d_player_y = 1.0;
            }
            if controller.button(MoveLeft).ended_down {
                d_player_x = -1.0;
            }
            if controller.button(MoveRight).ended_down {
                d_player_x = 1.0;
            }

            d_player_x *= 64.0;
            d_player_y *= 64.0;

            // TODO(aalhendi): diagonal will be faster! fis once we have vectors
            game_state.player_x += input.dt_for_frame as f32 * d_player_x;
            game_state.player_y += input.dt_for_frame as f32 * d_player_y;
        }
    }

    let tilemap: [[u32; 16]; 9] = [
        [0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
        [0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
        [0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
        [0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
        [0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
        [0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
    ];

    draw_rectangle(
        buffer,
        0.0,
        0.0,
        buffer.width as f32,
        buffer.height as f32,
        1.0,
        0.0,
        1.0,
    );

    let tile_width = 60_f32;
    let tile_height = 60_f32;

    for row in 0..9 {
        for column in 0..16 {
            let color = if tilemap[row][column] == 1 {
                (1.0, 1.0, 1.0)
            } else {
                (0.5, 0.5, 0.5)
            };
            let min_x = column as f32 * tile_width;
            let min_y = row as f32 * tile_height;
            let max_x = min_x + tile_width;
            let max_y = min_y + tile_height;
            draw_rectangle(
                buffer, min_x, min_y, max_x, max_y, color.0, color.1, color.2,
            );
        }
    }

    let player_rgb = (1.0, 1.0, 0.0);
    let player_width = 0.75 * tile_width;
    let player_height = 0.75 * tile_height;
    let player_left = game_state.player_x - player_width / 2.0;
    let player_top = game_state.player_y - player_height;
    draw_rectangle(
        buffer,
        player_left,
        player_top,
        player_left + player_width,
        player_top + player_height,
        player_rgb.0,
        player_rgb.1,
        player_rgb.2,
    );
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
    f_min_x: f32,
    f_min_y: f32,
    f_max_x: f32,
    f_max_y: f32,
    r: f32,
    g: f32,
    b: f32,
) {
    let min_x = (f_min_x.round() as i32).clamp(0, buffer.width);
    let min_y = (f_min_y.round() as i32).clamp(0, buffer.height);
    let max_x = (f_max_x.round() as i32).clamp(0, buffer.width);
    let max_y = (f_max_y.round() as i32).clamp(0, buffer.height);

    if min_x >= max_x || min_y >= max_y {
        return;
    }

    let color = ((r * 255.0).round() as u32) << 16
        | ((g * 255.0).round() as u32) << 8
        | ((b * 255.0).round() as u32);

    let mut row = unsafe {
        buffer
            .memory
            .cast::<u8>()
            .add((min_x * buffer.bytes_per_pixel) as usize)
            .offset((min_y * buffer.pitch) as isize)
    };
    for _y in min_y..max_y {
        let mut pixel = row.cast::<u32>();
        for _x in min_x..max_x {
            unsafe {
                *pixel = color;
                pixel = pixel.add(1);
            };
        }
        row = unsafe { row.add(buffer.pitch as usize) };
    }
}
