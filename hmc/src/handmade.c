#include "handmade.h"

internal void game_output_sound(GameState *state, GameSoundOutputBuffer *sound_buffer, int tone_hz) {
    i16 tone_volume = 3000;
    int wave_period = sound_buffer->samples_per_second / tone_hz;

    i16 *sample_out = sound_buffer->samples;
    for (int sample_idx = 0; sample_idx < sound_buffer->sample_count; ++sample_idx) {
        i16 sample_value = 0;
        *sample_out++ = sample_value;
        *sample_out++ = sample_value;
    }
}

internal i32 round_f32_to_i32(f32 value) {
    // TODO(aalhendi): intrinsic?
    i32 res = (i32)(value + 0.5f);
    return res;
}

// TODO(aalhendi): generic/overloading?
internal u32 round_f32_to_u32(f32 value) {
    // TODO(aalhendi): intrinsic?
    u32 res = (u32)(value + 0.5f);
    return res;
}

internal void draw_rectangle(GameOffscreenBuffer *buff, f32 f_min_x, f32 f_min_y, f32 f_max_x, f32 f_max_y, f32 r,
                             f32 g, f32 b) {
    i32 min_x = round_f32_to_i32(f_min_x);
    i32 min_y = round_f32_to_i32(f_min_y);
    i32 max_x = round_f32_to_i32(f_max_x);
    i32 max_y = round_f32_to_i32(f_max_y);

    if (min_x < 0) {
        min_x = 0;
    }

    if (min_y < 0) {
        min_y = 0;
    }

    if (max_x > buff->width) {
        max_x = buff->width;
    }

    if (max_y > buff->height) {
        max_y = buff->height;
    }

    u32 color = ((round_f32_to_u32(r * 255.0f) << 16) | (round_f32_to_u32(g * 255.0f) << 8) |
                 (round_f32_to_u32(b * 255.0f) << 0));

    u8 *row = ((u8 *)buff->memory + min_x * buff->bytes_per_pixel + min_y * buff->pitch);
    for (int y = min_y; y < max_y; ++y) {
        u32 *pixel = (u32 *)row;
        for (int x = min_x; x < max_x; ++x) {
            *pixel++ = color;
        }

        row += buff->pitch;
    }
}

extern GAME_UPDATE_AND_RENDER(game_update_and_render) {
    Assert((&input->controllers[0].terminator - &input->controllers[0].buttons[0]) ==
           (ArrayCount(input->controllers[0].buttons)));
    Assert(sizeof(GameState) <= memory->permanent_storage_size);

    GameState *game_state = (GameState *)memory->permanent_storage;
    if (!memory->is_initialized) {
        memory->is_initialized = true;
    }

    for (int controller_idx = 0; controller_idx < ArrayCount(input->controllers); ++controller_idx) {
        GameControllerInput *controller = get_controller(input, controller_idx);
        if (controller->is_analog) {
            // NOTE(aalhendi): Use analog movement tuning
        } else {
            // NOTE(aalhendi): Use digital movement tuning
            f32 d_player_x = 0.0f; // pixels/second
            f32 d_player_y = 0.0f; // pixels/second

            if (controller->move_up.ended_down) {
                d_player_y = -1.0f;
            }
            if (controller->move_down.ended_down) {
                d_player_y = 1.0f;
            }
            if (controller->move_left.ended_down) {
                d_player_x = -1.0f;
            }
            if (controller->move_right.ended_down) {
                d_player_x = 1.0f;
            }
            d_player_x *= 64.0f;
            d_player_y *= 64.0f;

            // TODO(aalhendi): diagonal movement faster... vectors will fix :)
            game_state->player_x += input->dt_for_frame * d_player_x;
            game_state->player_y += input->dt_for_frame * d_player_y;
        }
    }

    u32 tile_map[9][17] = {
        {1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1}, {1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1},
        {1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1}, {1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1},
        {0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0}, {1, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1},
        {1, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1}, {1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1},
        {1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1},
    };

    f32 upper_left_x = -30;
    f32 upper_left_y = 0;
    f32 tile_width = 60;
    f32 tile_height = 60;

    draw_rectangle(buff, 0.0f, 0.0f, (f32)buff->width, (f32)buff->height, 1.0f, 0.0f, 0.1f);
    for (int row = 0; row < 9; ++row) {
        for (int column = 0; column < 17; ++column) {
            u32 tile_id = tile_map[row][column];
            f32 gray = 0.5f;
            if (tile_id == 1) {
                gray = 1.0f;
            }

            f32 min_x = upper_left_x + ((f32)column) * tile_width;
            f32 min_y = upper_left_y + ((f32)row) * tile_height;
            f32 max_x = min_x + tile_width;
            f32 max_y = min_y + tile_height;
            draw_rectangle(buff, min_x, min_y, max_x, max_y, gray, gray, gray);
        }
    }

    f32 player_r = 1.0f;
    f32 player_g = 1.0f;
    f32 player_b = 0.0f;
    f32 player_width = 0.75f * tile_width;
    f32 player_height = tile_height;
    f32 player_left = game_state->player_x - 0.5f * player_width;
    f32 player_top = game_state->player_y - player_height;
    draw_rectangle(buff, player_left, player_top, player_left + player_width, player_top + player_height, player_r,
                   player_g, player_b);
}

extern GAME_GET_SOUND_SAMPLES(game_get_sound_samples) {
    GameState *game_state = (GameState *)memory->permanent_storage;
    game_output_sound(game_state, sound_buffer, 400);
}
