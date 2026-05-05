#define SDL_MAIN_HANDLED
#include <SDL3/SDL.h>

#include "linux_sdl_handmade.h"

#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

global_variable bool32 GLOBAL_RUNNING;
global_variable bool32 GLOBAL_PAUSE;
global_variable LinuxOffscreenBuffer GLOBAL_BACKBUFFER;
global_variable u64 GLOBAL_PERF_COUNT_FREQUENCY;

internal void linux_log(char *format, ...) {
    va_list args;
    va_start(args, format);
    vfprintf(stderr, format, args);
    va_end(args);
}

internal void linux_cat_path(char *base, char *filename, size_t dst_count, char *dst) {
    int written = snprintf(dst, dst_count, "%s%s", base, filename);
    Assert(written >= 0 && (size_t)written < dst_count);
}

internal bool32 linux_get_exe_dir(LinuxState *state) {
    bool32 result = false;

    char exe_filename[LINUX_MAX_PATH];
    ssize_t filename_size = readlink("/proc/self/exe", exe_filename, sizeof(exe_filename) - 1);
    if (filename_size > 0 && filename_size < (ssize_t)(sizeof(exe_filename) - 1)) {
        exe_filename[filename_size] = 0;

        ssize_t last_slash = -1;
        for (ssize_t idx = filename_size - 1; idx >= 0; --idx) {
            if (exe_filename[idx] == '/') {
                last_slash = idx;
                break;
            }
        }

        if (last_slash >= 0) {
            size_t dir_size = (size_t)last_slash + 1;
            Assert(dir_size < sizeof(state->exe_dir));
            memcpy(state->exe_dir, exe_filename, dir_size);
            state->exe_dir[dir_size] = 0;
            result = true;
        }
    }

    return result;
}

internal u64 linux_get_wall_clock(void) {
    u64 result = SDL_GetPerformanceCounter();
    return result;
}

internal f32 linux_get_seconds_elapsed(u64 start, u64 end) {
    f32 result = ((f32)(end - start) / (f32)GLOBAL_PERF_COUNT_FREQUENCY);
    return result;
}

internal struct timespec linux_get_last_write_time(char *filename) {
    struct timespec result = {0};

    struct stat attributes;
    if (stat(filename, &attributes) == 0) {
        result = attributes.st_mtim;
    }

    return result;
}

internal bool32 linux_timespecs_are_different(struct timespec a, struct timespec b) {
    bool32 result = ((a.tv_sec != b.tv_sec) || (a.tv_nsec != b.tv_nsec));
    return result;
}

internal bool32 linux_copy_file(char *source_filename, char *dest_filename) {
    bool32 result = false;

    int source = open(source_filename, O_RDONLY);
    if (source >= 0) {
        int dest = open(dest_filename, O_WRONLY | O_CREAT | O_TRUNC, 0755);
        if (dest >= 0) {
            result = true;

            char buffer[16 * 1024];
            for (;;) {
                ssize_t bytes_read = read(source, buffer, sizeof(buffer));
                if (bytes_read == 0) {
                    break;
                }

                if (bytes_read < 0) {
                    result = false;
                    break;
                }

                ssize_t bytes_written_total = 0;
                while (bytes_written_total < bytes_read) {
                    ssize_t bytes_written =
                        write(dest, buffer + bytes_written_total, (size_t)(bytes_read - bytes_written_total));
                    if (bytes_written <= 0) {
                        result = false;
                        break;
                    }
                    bytes_written_total += bytes_written;
                }

                if (!result) {
                    break;
                }
            }

            close(dest);
        }

        close(source);
    }

    return result;
}

internal LinuxGameCode linux_load_game_code(char *source_so_name, char *temp_so_name) {
    LinuxGameCode result = {0};
    struct timespec source_last_write_time = linux_get_last_write_time(source_so_name);

    if (linux_copy_file(source_so_name, temp_so_name)) {
        result.game_code_so = dlopen(temp_so_name, RTLD_NOW | RTLD_LOCAL);
        if (result.game_code_so) {
            result.update_and_render = (GameUpdateAndRender *)dlsym(result.game_code_so, "game_update_and_render");
            result.get_sound_samples = (GameGetSoundSamples *)dlsym(result.game_code_so, "game_get_sound_samples");
            result.is_valid = (result.update_and_render && result.get_sound_samples);
            if (result.is_valid) {
                result.last_write_time = source_last_write_time;
            }
        } else {
            linux_log("dlopen failed: %s\n", (char *)dlerror());
        }
    }

    if (!result.is_valid) {
        if (result.game_code_so) {
            dlclose(result.game_code_so);
            result.game_code_so = 0;
        }
        result.update_and_render = 0;
        result.get_sound_samples = 0;
    }

    return result;
}

internal void linux_unload_game_code(LinuxGameCode *game_code) {
    if (game_code->game_code_so) {
        dlclose(game_code->game_code_so);
        game_code->game_code_so = 0;
    }

    game_code->is_valid = false;
    game_code->update_and_render = 0;
    game_code->get_sound_samples = 0;
}

internal void linux_resize_offscreen_buffer(LinuxOffscreenBuffer *buffer, i32 width, i32 height) {
    if (buffer->memory) {
        free(buffer->memory);
    }

    buffer->width = width;
    buffer->height = height;
    buffer->bytes_per_pixel = 4;
    buffer->pitch = buffer->width * buffer->bytes_per_pixel;

    size_t bitmap_memory_size = (size_t)buffer->pitch * (size_t)buffer->height;
    buffer->memory = malloc(bitmap_memory_size);
}

internal SDL_Texture *linux_create_texture(SDL_Renderer *renderer, LinuxOffscreenBuffer *buffer) {
    SDL_Texture *texture = SDL_CreateTexture(renderer, SDL_PIXELFORMAT_XRGB8888, SDL_TEXTUREACCESS_STREAMING,
                                             buffer->width, buffer->height);
    if (texture) {
        SDL_SetTextureScaleMode(texture, SDL_SCALEMODE_NEAREST);
    }
    return texture;
}

internal void linux_display_buffer(SDL_Renderer *renderer, SDL_Texture *texture, LinuxOffscreenBuffer *buffer) {
    if (buffer->memory && texture) {
        SDL_UpdateTexture(texture, 0, buffer->memory, buffer->pitch);
        SDL_RenderClear(renderer);
        SDL_RenderTexture(renderer, texture, 0, 0);
        SDL_RenderPresent(renderer);
    }
}

#if DEBUG_PROFILE
internal DEBUG_PLATFORM_FREE_FILE_MEMORY(debug_platform_free_file_memory) {
    if (memory) {
        free(memory);
    }
}

internal DEBUG_PLATFORM_READ_ENTIRE_FILE(debug_platform_read_entire_file) {
    DebugFileReadResult result = {0};

    int file = open(filename, O_RDONLY);
    if (file >= 0) {
        struct stat file_stat;
        if (fstat(file, &file_stat) == 0) {
            u32 file_size = truncate_u64_to_u32_safe((u64)file_stat.st_size);
            result.contents = malloc(file_size);
            if (result.contents) {
                u8 *dst = (u8 *)result.contents;
                u32 bytes_remaining = file_size;
                while (bytes_remaining > 0) {
                    ssize_t bytes_read = read(file, dst, bytes_remaining);
                    if (bytes_read <= 0) {
                        debug_platform_free_file_memory(thread, result.contents);
                        result.contents = 0;
                        break;
                    }

                    bytes_remaining -= (u32)bytes_read;
                    dst += bytes_read;
                }

                if (result.contents) {
                    result.contents_size = file_size;
                }
            }
        }

        close(file);
    }

    return result;
}

internal DEBUG_PLATFORM_WRITE_ENTIRE_FILE(debug_platform_write_entire_file) {
    bool32 result = false;

    int file = open(filename, O_WRONLY | O_CREAT | O_TRUNC, 0666);
    if (file >= 0) {
        u8 *src = (u8 *)memory;
        u32 bytes_remaining = memory_size;
        result = true;

        while (bytes_remaining > 0) {
            ssize_t bytes_written = write(file, src, bytes_remaining);
            if (bytes_written <= 0) {
                result = false;
                break;
            }

            bytes_remaining -= (u32)bytes_written;
            src += bytes_written;
        }

        close(file);
    }

    return result;
}
#endif

internal LinuxReplayBuffer *linux_get_replay_buffer(LinuxState *state, int unsigned index) {
    Assert(index < ArrayCount(state->replay_buffers));
    LinuxReplayBuffer *result = &state->replay_buffers[index];
    return result;
}

internal void linux_get_input_file_location(LinuxState *state, bool32 input_stream, int slot_idx, size_t dst_count,
                                            char *dst) {
    char filename[64];
    snprintf(filename, sizeof(filename), "loop_edit_%d_%s.hmi", slot_idx, input_stream ? "input" : "state");
    linux_cat_path(state->exe_dir, filename, dst_count, dst);
}

internal void linux_begin_recording_input(LinuxState *state, int input_recording_idx) {
    LinuxReplayBuffer *replay_buffer = linux_get_replay_buffer(state, input_recording_idx);
    if (replay_buffer->memory) {
        char filename[LINUX_MAX_PATH];
        linux_get_input_file_location(state, true, input_recording_idx, sizeof(filename), filename);
        state->recording_file = fopen(filename, "wb");
        if (state->recording_file) {
            state->input_recording_idx = input_recording_idx;
            memcpy(replay_buffer->memory, state->memory, (size_t)state->total_size);
        }
    }
}

internal void linux_end_recording_input(LinuxState *state) {
    if (state->recording_file) {
        fclose(state->recording_file);
        state->recording_file = 0;
    }
    state->input_recording_idx = 0;
}

internal void linux_begin_input_playback(LinuxState *state, int input_playing_idx) {
    LinuxReplayBuffer *replay_buffer = linux_get_replay_buffer(state, input_playing_idx);
    if (replay_buffer->memory) {
        char filename[LINUX_MAX_PATH];
        linux_get_input_file_location(state, true, input_playing_idx, sizeof(filename), filename);
        state->playback_file = fopen(filename, "rb");
        if (state->playback_file) {
            state->input_playing_idx = input_playing_idx;
            memcpy(state->memory, replay_buffer->memory, (size_t)state->total_size);
        }
    }
}

internal void linux_end_input_playback(LinuxState *state) {
    if (state->playback_file) {
        fclose(state->playback_file);
        state->playback_file = 0;
    }
    state->input_playing_idx = 0;
}

internal void linux_record_input(LinuxState *state, GameInput *new_input) {
    if (state->recording_file) {
        fwrite(new_input, sizeof(*new_input), 1, state->recording_file);
    }
}

internal void linux_playback_input(LinuxState *state, GameInput *new_input) {
    if (state->playback_file) {
        size_t read_count = fread(new_input, sizeof(*new_input), 1, state->playback_file);
        if (read_count == 0) {
            int playing_idx = state->input_playing_idx;
            linux_end_input_playback(state);
            linux_begin_input_playback(state, playing_idx);
            if (state->playback_file) {
                fread(new_input, sizeof(*new_input), 1, state->playback_file);
            }
        }
    }
}

internal void linux_process_keyboard_message(GameButtonState *new_state, bool32 is_down) {
    if (new_state->ended_down != is_down) {
        new_state->ended_down = is_down;
        ++new_state->half_transition_count;
    }
}

internal void linux_preserve_button_state(GameButtonState *new_buttons, GameButtonState *old_buttons,
                                          int unsigned button_count) {
    for (int unsigned button_idx = 0; button_idx < button_count; ++button_idx) {
        new_buttons[button_idx] = (GameButtonState){0};
        new_buttons[button_idx].ended_down = old_buttons[button_idx].ended_down;
    }
}

internal void linux_process_pending_events(LinuxState *state, GameControllerInput *keyboard_controller) {
    SDL_Event event;
    while (SDL_PollEvent(&event)) {
        switch (event.type) {
        case SDL_EVENT_QUIT: {
            GLOBAL_RUNNING = false;
        } break;

        case SDL_EVENT_KEY_DOWN:
        case SDL_EVENT_KEY_UP: {
            if (event.key.repeat) {
                break;
            }

            bool32 is_down = event.key.down;
            switch (event.key.scancode) {
            case SDL_SCANCODE_W: {
                linux_process_keyboard_message(&keyboard_controller->move_up, is_down);
            } break;
            case SDL_SCANCODE_A: {
                linux_process_keyboard_message(&keyboard_controller->move_left, is_down);
            } break;
            case SDL_SCANCODE_S: {
                linux_process_keyboard_message(&keyboard_controller->move_down, is_down);
            } break;
            case SDL_SCANCODE_D: {
                linux_process_keyboard_message(&keyboard_controller->move_right, is_down);
            } break;
            case SDL_SCANCODE_Q: {
                linux_process_keyboard_message(&keyboard_controller->left_shoulder, is_down);
            } break;
            case SDL_SCANCODE_E: {
                linux_process_keyboard_message(&keyboard_controller->right_shoulder, is_down);
            } break;
            case SDL_SCANCODE_UP: {
                linux_process_keyboard_message(&keyboard_controller->action_up, is_down);
            } break;
            case SDL_SCANCODE_LEFT: {
                linux_process_keyboard_message(&keyboard_controller->action_left, is_down);
            } break;
            case SDL_SCANCODE_DOWN: {
                linux_process_keyboard_message(&keyboard_controller->action_down, is_down);
            } break;
            case SDL_SCANCODE_RIGHT: {
                linux_process_keyboard_message(&keyboard_controller->action_right, is_down);
            } break;
            case SDL_SCANCODE_ESCAPE: {
                linux_process_keyboard_message(&keyboard_controller->start, is_down);
            } break;
            case SDL_SCANCODE_SPACE: {
                linux_process_keyboard_message(&keyboard_controller->back, is_down);
            } break;
#if DEBUG_PROFILE
            case SDL_SCANCODE_P: {
                if (is_down) {
                    GLOBAL_PAUSE = !GLOBAL_PAUSE;
                }
            } break;
            case SDL_SCANCODE_L: {
                if (is_down) {
                    if (state->input_playing_idx == 0) {
                        if (state->input_recording_idx == 0) {
                            linux_begin_recording_input(state, 1);
                        } else {
                            linux_end_recording_input(state);
                            linux_begin_input_playback(state, 1);
                        }
                    } else {
                        linux_end_input_playback(state);
                    }
                }
            } break;
#endif
            case SDL_SCANCODE_F4: {
                if (is_down && (event.key.mod & SDL_KMOD_ALT)) {
                    GLOBAL_RUNNING = false;
                }
            } break;
            default: {
            } break;
            }
        } break;

        default: {
        } break;
        }
    }
}

internal void linux_update_mouse_input(GameInput *new_input, GameInput *old_input) {
    float mouse_x;
    float mouse_y;
    SDL_MouseButtonFlags mouse_state = SDL_GetMouseState(&mouse_x, &mouse_y);

    new_input->mouse_x = (i32)mouse_x;
    new_input->mouse_y = (i32)mouse_y;
    new_input->mouse_z = 0;

    linux_preserve_button_state(new_input->mouse_buttons, old_input->mouse_buttons, ArrayCount(new_input->mouse_buttons));
    linux_process_keyboard_message(&new_input->mouse_buttons[0], (mouse_state & SDL_BUTTON_LMASK) != 0);
    linux_process_keyboard_message(&new_input->mouse_buttons[1], (mouse_state & SDL_BUTTON_MMASK) != 0);
    linux_process_keyboard_message(&new_input->mouse_buttons[2], (mouse_state & SDL_BUTTON_RMASK) != 0);
    linux_process_keyboard_message(&new_input->mouse_buttons[3], (mouse_state & SDL_BUTTON_X1MASK) != 0);
    linux_process_keyboard_message(&new_input->mouse_buttons[4], (mouse_state & SDL_BUTTON_X2MASK) != 0);
}

internal void linux_fill_audio_stream(SDL_AudioStream *audio_stream, LinuxSoundOutput *sound_output,
                                      GameGetSoundSamples *get_sound_samples, ThreadContext *thread,
                                      GameMemory *game_memory, i16 *samples) {
    if (audio_stream && get_sound_samples) {
        int queued_bytes = SDL_GetAudioStreamQueued(audio_stream);
        if (queued_bytes >= 0 && queued_bytes < sound_output->target_queue_bytes) {
            int bytes_to_write = sound_output->target_queue_bytes - queued_bytes;
            bytes_to_write -= bytes_to_write % sound_output->bytes_per_sample;

            GameSoundOutputBuffer sound_buffer = {0};
            sound_buffer.samples_per_second = sound_output->samples_per_second;
            sound_buffer.sample_count = bytes_to_write / sound_output->bytes_per_sample;
            sound_buffer.samples = samples;

            get_sound_samples(thread, game_memory, &sound_buffer);
            SDL_PutAudioStreamData(audio_stream, samples, bytes_to_write);
            sound_output->running_sample_index += sound_buffer.sample_count;
        }
    }
}

int main(int argc, char **argv) {
    (void)argc;
    (void)argv;

    LinuxState state = {0};
    if (!linux_get_exe_dir(&state)) {
        linux_log("Failed to discover executable directory.\n");
        return 1;
    }

    if (!SDL_Init(SDL_INIT_VIDEO | SDL_INIT_AUDIO | SDL_INIT_GAMEPAD)) {
        linux_log("SDL_Init failed: %s\n", SDL_GetError());
        return 1;
    }

    GLOBAL_PERF_COUNT_FREQUENCY = SDL_GetPerformanceFrequency();

    linux_resize_offscreen_buffer(&GLOBAL_BACKBUFFER, 960, 540);

    SDL_Window *window = SDL_CreateWindow("Handmade Hero", GLOBAL_BACKBUFFER.width, GLOBAL_BACKBUFFER.height,
                                          SDL_WINDOW_RESIZABLE);
    if (!window) {
        linux_log("SDL_CreateWindow failed: %s\n", SDL_GetError());
        SDL_Quit();
        return 1;
    }

    SDL_Renderer *renderer = SDL_CreateRenderer(window, 0);
    if (!renderer) {
        linux_log("SDL_CreateRenderer failed: %s\n", SDL_GetError());
        SDL_DestroyWindow(window);
        SDL_Quit();
        return 1;
    }

    SDL_Texture *texture = linux_create_texture(renderer, &GLOBAL_BACKBUFFER);
    if (!texture) {
        linux_log("SDL_CreateTexture failed: %s\n", SDL_GetError());
        SDL_DestroyRenderer(renderer);
        SDL_DestroyWindow(window);
        SDL_Quit();
        return 1;
    }

    char source_so_name[LINUX_MAX_PATH];
    char temp_so_names[2][LINUX_MAX_PATH];
    linux_cat_path(state.exe_dir, "handmade.so", sizeof(source_so_name), source_so_name);
    linux_cat_path(state.exe_dir, "handmade_temp_0.so", sizeof(temp_so_names[0]), temp_so_names[0]);
    linux_cat_path(state.exe_dir, "handmade_temp_1.so", sizeof(temp_so_names[1]), temp_so_names[1]);

    f32 game_update_hz = 30.0f;
    f32 target_seconds_per_frame = 1.0f / game_update_hz;

    LinuxSoundOutput sound_output = {
        .samples_per_second = 48000,
        .bytes_per_sample = sizeof(i16) * 2,
    };
    sound_output.buffer_size = sound_output.samples_per_second * sound_output.bytes_per_sample;
    sound_output.target_queue_bytes = sound_output.buffer_size / 15;

    SDL_AudioSpec audio_spec = {
        .format = SDL_AUDIO_S16,
        .channels = 2,
        .freq = sound_output.samples_per_second,
    };
    SDL_AudioStream *audio_stream =
        SDL_OpenAudioDeviceStream(SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK, &audio_spec, 0, 0);
    if (audio_stream) {
        SDL_ResumeAudioStreamDevice(audio_stream);
    } else {
        linux_log("Audio disabled: %s\n", SDL_GetError());
    }

    i16 *samples = (i16 *)malloc((size_t)sound_output.buffer_size);
    if (!samples) {
        linux_log("Audio disabled: failed to allocate sample buffer.\n");
        if (audio_stream) {
            SDL_DestroyAudioStream(audio_stream);
            audio_stream = 0;
        }
    }

    GameMemory game_memory = {
        .permanent_storage_size = Megabytes(64),
        .transient_storage_size = Gigabytes(1),
#if DEBUG_PROFILE
        .debug_platform_free_file_memory = debug_platform_free_file_memory,
        .debug_platform_read_entire_file = debug_platform_read_entire_file,
        .debug_platform_write_entire_file = debug_platform_write_entire_file,
#endif
    };

#if DEBUG_PROFILE
    void *base_address = (void *)Terabytes(2);
#else
    void *base_address = 0;
#endif

    state.total_size = game_memory.permanent_storage_size + game_memory.transient_storage_size;
    state.memory = mmap(base_address, (size_t)state.total_size, PROT_READ | PROT_WRITE,
                        MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (state.memory == MAP_FAILED) {
        linux_log("Failed to allocate game memory: %s\n", strerror(errno));
        state.memory = 0;
    }

    if (state.memory) {
        game_memory.permanent_storage = state.memory;
        game_memory.transient_storage = (u8 *)game_memory.permanent_storage + game_memory.permanent_storage_size;
    }

    for (int replay_idx = 0; state.memory && replay_idx < ArrayCount(state.replay_buffers); ++replay_idx) {
        LinuxReplayBuffer *replay_buffer = &state.replay_buffers[replay_idx];
        linux_get_input_file_location(&state, false, replay_idx, sizeof(replay_buffer->filename),
                                      replay_buffer->filename);
        replay_buffer->memory = mmap(0, (size_t)state.total_size, PROT_READ | PROT_WRITE,
                                     MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (replay_buffer->memory == MAP_FAILED) {
            replay_buffer->memory = 0;
        }
    }

    if (GLOBAL_BACKBUFFER.memory && game_memory.permanent_storage && game_memory.transient_storage) {
        GameInput input[2] = {0};
        GameInput *new_input = &input[0];
        GameInput *old_input = &input[1];

        LinuxGameCode game = linux_load_game_code(source_so_name, temp_so_names[0]);
        u32 load_counter = 1;

        u64 last_counter = linux_get_wall_clock();
        GLOBAL_RUNNING = true;

        while (GLOBAL_RUNNING) {
            new_input->dt_for_frame = target_seconds_per_frame;

            struct timespec new_so_write_time = linux_get_last_write_time(source_so_name);
            if (!game.is_valid || linux_timespecs_are_different(new_so_write_time, game.last_write_time)) {
                LinuxGameCode new_game = linux_load_game_code(source_so_name, temp_so_names[load_counter++ & 1]);
                if (new_game.is_valid) {
                    linux_unload_game_code(&game);
                    game = new_game;
                } else {
                    linux_unload_game_code(&new_game);
                }
            }

            GameControllerInput *old_keyboard_controller = get_controller(old_input, 0);
            GameControllerInput *new_keyboard_controller = get_controller(new_input, 0);
            *new_keyboard_controller = (GameControllerInput){0};
            new_keyboard_controller->is_connected = true;
            linux_preserve_button_state(new_keyboard_controller->buttons, old_keyboard_controller->buttons,
                                        ArrayCount(new_keyboard_controller->buttons));

            linux_process_pending_events(&state, new_keyboard_controller);

            if (!GLOBAL_PAUSE) {
                linux_update_mouse_input(new_input, old_input);

                ThreadContext thread = {0};

                GameOffscreenBuffer buffer = {0};
                buffer.memory = GLOBAL_BACKBUFFER.memory;
                buffer.width = GLOBAL_BACKBUFFER.width;
                buffer.height = GLOBAL_BACKBUFFER.height;
                buffer.pitch = GLOBAL_BACKBUFFER.pitch;
                buffer.bytes_per_pixel = GLOBAL_BACKBUFFER.bytes_per_pixel;

                if (state.input_recording_idx) {
                    linux_record_input(&state, new_input);
                }

                if (state.input_playing_idx) {
                    linux_playback_input(&state, new_input);
                }

                if (game.update_and_render) {
                    game.update_and_render(&thread, &game_memory, new_input, &buffer);
                }

                linux_fill_audio_stream(audio_stream, &sound_output, game.get_sound_samples, &thread, &game_memory,
                                        samples);

                u64 work_counter = linux_get_wall_clock();
                f32 seconds_elapsed_for_frame = linux_get_seconds_elapsed(last_counter, work_counter);
                if (seconds_elapsed_for_frame < target_seconds_per_frame) {
                    u32 sleep_ms = (u32)(1000.0f * (target_seconds_per_frame - seconds_elapsed_for_frame));
                    if (sleep_ms > 0) {
                        SDL_Delay(sleep_ms);
                    }

                    while (seconds_elapsed_for_frame < target_seconds_per_frame) {
                        seconds_elapsed_for_frame = linux_get_seconds_elapsed(last_counter, linux_get_wall_clock());
                    }
                }

                u64 end_counter = linux_get_wall_clock();
                last_counter = end_counter;

                linux_display_buffer(renderer, texture, &GLOBAL_BACKBUFFER);

                GameInput *temp = new_input;
                new_input = old_input;
                old_input = temp;
            }
        }

        linux_unload_game_code(&game);
    }

    if (state.recording_file) {
        linux_end_recording_input(&state);
    }
    if (state.playback_file) {
        linux_end_input_playback(&state);
    }
    for (int replay_idx = 0; replay_idx < ArrayCount(state.replay_buffers); ++replay_idx) {
        if (state.replay_buffers[replay_idx].memory) {
            munmap(state.replay_buffers[replay_idx].memory, (size_t)state.total_size);
        }
    }
    if (state.memory) {
        munmap(state.memory, (size_t)state.total_size);
    }
    free(samples);
    if (audio_stream) {
        SDL_DestroyAudioStream(audio_stream);
    }
    SDL_DestroyTexture(texture);
    SDL_DestroyRenderer(renderer);
    SDL_DestroyWindow(window);
    SDL_Quit();

    return 0;
}
