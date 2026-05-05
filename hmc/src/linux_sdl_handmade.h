#if !defined(LINUX_SDL_HANDMADE_H)

#include "handmade.h"

#include <stdio.h>
#include <time.h>

#define LINUX_MAX_PATH 4096

typedef struct LinuxOffscreenBuffer {
    void *memory;
    i32 width;
    i32 height;
    i32 pitch;
    i32 bytes_per_pixel;
} LinuxOffscreenBuffer;

typedef struct LinuxReplayBuffer {
    char filename[LINUX_MAX_PATH];
    void *memory;
} LinuxReplayBuffer;

typedef struct LinuxState {
    u64 total_size;
    void *memory;
    LinuxReplayBuffer replay_buffers[4];
    FILE *recording_file;
    FILE *playback_file;
    i32 input_playing_idx;
    i32 input_recording_idx;
    char exe_dir[LINUX_MAX_PATH];
} LinuxState;

typedef struct LinuxSoundOutput {
    i32 samples_per_second;
    i32 running_sample_index;
    i32 bytes_per_sample;
    i32 buffer_size;
    i32 target_queue_bytes;
} LinuxSoundOutput;

typedef struct LinuxGameCode {
    void *game_code_so;
    struct timespec last_write_time;
    GameUpdateAndRender *update_and_render;
    GameGetSoundSamples *get_sound_samples;
    bool32 is_valid;
} LinuxGameCode;

#define LINUX_SDL_HANDMADE_H
#endif
