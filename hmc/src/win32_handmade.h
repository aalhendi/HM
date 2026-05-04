#if !defined(WIN32_HANDMADE_H)

#include "handmade.h"
#include <windows.h>

typedef struct Win32DebugTimeMarker {
    DWORD output_play_cursor;
    DWORD output_write_cursor;
    DWORD output_location;
    DWORD output_byte_count;
    DWORD expected_flip_play_cursor;

    DWORD flip_play_cursor;
    DWORD flip_write_cursor;
} Win32DebugTimeMarker;

typedef struct Win32ReplayBuffer {
    HANDLE file_handle;
    HANDLE memory_map;
    // NOTE(aalhendi): Never use MAX_PATH in user-facing code. can be dangerous and lead to bad results.
    char filename[MAX_PATH];
    void *memory;
} Win32ReplayBuffer;

typedef struct Win32State {
    u64 total_size;
    void *memory;
    Win32ReplayBuffer replay_buffers[4];
    HANDLE recording_file_handle;
    i32 input_playing_idx;
    i32 input_recording_idx;
    HANDLE playback_file_handle;
    char exe_filename[MAX_PATH];
    u32 exe_filename_base_offset;
} Win32State;

typedef struct Win32WindowDimension {
    i32 width;
    i32 height;
} Win32WindowDimension;

typedef struct Win32OffscreenBuffer {
    // NOTE(aalhendi): 32-bit wide pixels, Memory Order BB GG RR XX
    BITMAPINFO info;
    // NOTE(aalhendi): void* to avoid specifying the type, we want windows to give us back a ptr to the bitmap memory
    //  windows doesn't know (on the API lvl), what sort of flags, and therefore what kind of memory we want.
    //  CreateDIBSection also can't haveThe fn can only have one signature, it can't get an u8ptr OR an u64 ptr etc. so
    //  we pass a void* and cast appropriately it is used as a double ptr because we give windows an addr of a ptr which
    //  we want it to OVERWRITE into a NEW PTR which would point to where it alloc'd mem
    void *memory;
    // NOTE(aalhendi): We store width and height in self.info.bmiHeader. This is redundant. Keeping because its only 8
    // bytes
    i32 width;
    i32 height;
    i32 pitch;
    i32 bytes_per_pixel;
} Win32OffscreenBuffer;

typedef struct Win32SoundOutput {
    i32 samples_per_second;
    i32 running_sample_index;
    i32 bytes_per_sample;
    DWORD buffer_size;
    DWORD safety_bytes;
    // TODO(aalhendi): adding a "bytes_per_second" would simplify math.
    // TODO(aalhendi): should runnning sample index be in bytes as well?
} Win32SoundOutput;

typedef struct Win32GameCode {
    HMODULE game_code_dll;
    FILETIME last_write_time;

    // TODO(aalhendi): fn types?
    GameUpdateAndRender *update_and_render;
    GameGetSoundSamples *get_sound_samples;

    bool32 is_valid;
} Win32GameCode;

#define WIN32_HANDMADE_H
#endif
