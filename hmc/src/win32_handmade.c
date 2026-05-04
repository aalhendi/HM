#include "win32_handmade.h"
#include "handmade.h"
#include <Xinput.h>
#include <dsound.h>
#include <windows.h>

/* TODO(aalhendi): THIS IS NOT A FINAL PLATFORM LAYER!!!

- Saved game locations
- Getting a handle to our own executable file
- Asset loading path
- Threading (launch a thread)
- Raw Input (support for multiple keyboards)
- Sleep/timeBeginPeriod
- ClipCursor() (for multimonitor support)
- Fullscreen support
- WM_SETCURSOR (control cursor visibility)
- QueryCancelAutoPlay()
- WM_ACTIVATEAPP (for when we are not the active application)
- Blit speed improvements (BitBlt)
- Hardware acceleration (OpenGL, Direct3d or BOTH??)
- GetKeyboardLayout() (for intl WASD support)

*/

i64 GLOBAL_PERF_COUNT_FREQUENCY;
global_variable bool32 GLOBAL_RUNNING;
global_variable bool32 GLOBAL_PAUSE;
global_variable Win32OffscreenBuffer GLOBAL_BACKBUFFER;
global_variable LPDIRECTSOUNDBUFFER GLOBAL_SECONDARY_BUFFER;

const i32 KEY_MESSAGE_WAS_DOWN_BIT = 30;
const i32 KEY_MESSAGE_IS_DOWN_BIT = 31;
const i32 KEY_MESSAGE_IS_ALT_BIT = 29;

/*
  NOTE(aalhendi): We dynamically load XInput to avoid static linking to the Windows
  import library. This prevents the .exe from crashing on startup if xinput1_4.dll
  is missing on the user's machine.
*/
typedef DWORD WINAPI XInputGetStateFn(DWORD dwUserIndex, XINPUT_STATE *pState);
typedef DWORD WINAPI XInputSetStateFn(DWORD dwUserIndex, XINPUT_VIBRATION *pVibration);

// NOTE(aalhendi): define & point to stubs so fn ptrs are valid even if lib loading fails. so calls them are branchless
DWORD WINAPI xinput_get_state_stub(DWORD dwUserIndex, XINPUT_STATE *pState) { return ERROR_DEVICE_NOT_CONNECTED; }
DWORD WINAPI xinput_set_state_stub(DWORD dwUserIndex, XINPUT_VIBRATION *pVibration) {
    return ERROR_DEVICE_NOT_CONNECTED;
}

global_variable XInputGetStateFn *xinput_get_state = xinput_get_state_stub;
global_variable XInputSetStateFn *xinput_set_state = xinput_set_state_stub;

// NOTE(aalhendi): hijack standard Windows API names
#define XInputGetState xinput_get_state
#define XInputSetState xinput_set_state

typedef HRESULT WINAPI DirectSoundCreateFn(LPCGUID pcGuidDevice, LPDIRECTSOUND *ppDS, LPUNKNOWN pUnkOuter);

internal void win32_load_xinput(void) {
    HMODULE xinput_library = LoadLibraryA("xinput1_4.dll");
    if (!xinput_library) {
        xinput_library = LoadLibraryA("xinput9_1_0.dll");
    }
    if (!xinput_library) {
        xinput_library = LoadLibraryA("xinput1_3.dll");
    }

    if (xinput_library) {
        xinput_get_state = (XInputGetStateFn *)GetProcAddress(xinput_library, "XInputGetState");
        if (!xinput_get_state) {
            xinput_get_state = xinput_get_state_stub;
        }

        xinput_set_state = (XInputSetStateFn *)GetProcAddress(xinput_library, "XInputSetState");
        if (!xinput_set_state) {
            xinput_set_state = xinput_set_state_stub;
        }
    }
}

internal void cat_strings(size_t src_a_cnt, char *src_a, size_t src_b_cnt, char *src_b, size_t dst_cnt, char *dst) {
    Assert(dst_cnt >= (src_a_cnt + src_b_cnt + 1)); // NOTE(aalhendi): accounts for null terminator

    for (size_t idx = 0; idx < src_a_cnt; ++idx) {
        *dst++ = *src_a++;
    }

    for (size_t idx = 0; idx < src_b_cnt; ++idx) {
        *dst++ = *src_b++;
    }

    *dst++ = 0;
}

// TODO(aalhendi): eventually build a String8 which behaves like &str in Rust.
internal i32 string_length(char *string) {
    Assert(string != 0);
    i32 cnt = 0;
    while (*string++) {
        ++cnt;
    }
    return (cnt);
}

inline LARGE_INTEGER win32_get_wall_clock(void) {
    LARGE_INTEGER result;
    QueryPerformanceCounter(&result);
    return (result);
}

inline f32 win32_get_seconds_elapsed(LARGE_INTEGER start, LARGE_INTEGER end) {
    f32 result = ((f32)(end.QuadPart - start.QuadPart) / (f32)GLOBAL_PERF_COUNT_FREQUENCY);
    return (result);
}

inline FILETIME win32_get_last_write_time(char *filename) {
    FILETIME last_write_time = {0};

    WIN32_FILE_ATTRIBUTE_DATA data;
    if (GetFileAttributesEx(filename, GetFileExInfoStandard, &data)) {
        last_write_time = data.ftLastWriteTime;
    }

    return (last_write_time);
}

internal Win32GameCode win32_load_game_code(char *source_dll_name, char *temp_dll_name) {
    Win32GameCode result = {0};

    // TODO(aalhendi): proper path here
    // TODO(aalhendi): automatically determine when updates are necessary.

    FILETIME source_last_write_time = win32_get_last_write_time(source_dll_name);

    if (CopyFile(source_dll_name, temp_dll_name, FALSE)) {
        result.game_code_dll = LoadLibraryA(temp_dll_name);
        if (result.game_code_dll) {
            result.update_and_render =
                (GameUpdateAndRender *)GetProcAddress(result.game_code_dll, "game_update_and_render");

            result.get_sound_samples =
                (GameGetSoundSamples *)GetProcAddress(result.game_code_dll, "game_get_sound_samples");

            result.is_valid = (result.update_and_render && result.get_sound_samples);
            if (result.is_valid) {
                result.last_write_time = source_last_write_time;
            }
        }
    }

    if (!result.is_valid) {
        if (result.game_code_dll) {
            FreeLibrary(result.game_code_dll);
            result.game_code_dll = 0;
        }
        result.update_and_render = 0;
        result.get_sound_samples = 0;
    }

    return (result);
}

internal void win32_unload_game_code(Win32GameCode *game_code) {
    if (game_code->game_code_dll) {
        FreeLibrary(game_code->game_code_dll);
        game_code->game_code_dll = 0;
    }

    game_code->is_valid = false;
    game_code->update_and_render = 0;
    game_code->get_sound_samples = 0;
}

// TODO(aalhendi): can get cleaned up!
internal void win32_get_exe_filename(Win32State *state, HMODULE module_handle) {
    DWORD filename_size = GetModuleFileNameA(module_handle, state->exe_filename, sizeof(state->exe_filename));

    state->exe_filename_base_offset = 0;
    // NOTE(aalhendi): check for failure or partial path. They are both useless.
    if (filename_size == 0 || filename_size == sizeof(state->exe_filename)) {
        OutputDebugStringA("Error: GetModuleFileNameA failed");
        return;
    }

    for (i32 i = (i32)filename_size - 1; i >= 0; --i) {
        if (state->exe_filename[i] == '\\') {
            state->exe_filename_base_offset = (u32)i + 1;
            break;
        }
    }
}

internal void win32_build_exe_file_path_name(Win32State *state, char *filename, u32 dst_cnt, void *dst) {
    cat_strings(state->exe_filename_base_offset, state->exe_filename, string_length(filename), filename, dst_cnt, dst);
}

internal void win32_get_input_file_location(Win32State *state, bool32 input_stream, int slot_idx, int dst_cnt,
                                            char *dst) {
    char temp[64];
    wsprintf(temp, "loop_edit_%d_%s.hmi", slot_idx, input_stream ? "input" : "state");
    win32_build_exe_file_path_name(state, temp, dst_cnt, dst);
}

internal void win32_resize_dib_section(Win32OffscreenBuffer *buff, i32 width, i32 height) {
    if (buff->memory != NULL) {
        if (VirtualFree(buff->memory, 0, MEM_RELEASE) == FALSE) {
            OutputDebugStringA("Failed to free memory!");
        }
    }

    buff->width = width;
    buff->height = height;
    buff->bytes_per_pixel = 4;

    BITMAPINFOHEADER bmp_info_header = {
        .biSize = sizeof(BITMAPINFOHEADER),
        .biWidth = width,
        // NOTE(aalhendi): When bHeight is negative, it clues Windows to treat the bitmap as top-down rather than
        // bottom-up. This means that the first three bytes are for the top-left pixel.
        .biHeight = -height,
        .biPlanes = 1,
        .biBitCount = 32, // 8 for red, 8 for green, 8 for blue, ask for 32 for DWORD alignment
        .biCompression = BI_RGB,
        .biSizeImage = 0,
        .biXPelsPerMeter = 0,
        .biYPelsPerMeter = 0,
        .biClrUsed = 0,
        .biClrImportant = 0};

    buff->info.bmiHeader = bmp_info_header;

    SIZE_T bmp_memory_size = buff->bytes_per_pixel * buff->width * buff->height;
    buff->memory = VirtualAlloc(NULL, bmp_memory_size, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE);

    buff->pitch = buff->width * buff->bytes_per_pixel;
    // TODO(aalhendi): Probably clear this to blackFF
}

#if DEBUG_PROFILE
internal DEBUG_PLATFORM_FREE_FILE_MEMORY(debug_platform_free_file_memory) {
    if (memory) {
        VirtualFree(memory, 0, MEM_RELEASE);
    }
}

internal DEBUG_PLATFORM_READ_ENTIRE_FILE(debug_platform_read_entire_file) {
    DebugFileReadResult result = {0};

    HANDLE file_handle = CreateFileA(filename, GENERIC_READ, FILE_SHARE_READ, 0, OPEN_EXISTING, 0, 0);
    if (file_handle != INVALID_HANDLE_VALUE) {
        LARGE_INTEGER file_size;
        if (GetFileSizeEx(file_handle, &file_size)) {
            u32 file_size_32 = truncate_u64_to_u32_safe(file_size.QuadPart);
            result.contents = VirtualAlloc(0, file_size_32, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE);
            if (result.contents) {
                DWORD bytes_read;
                if (ReadFile(file_handle, result.contents, file_size_32, &bytes_read, 0) &&
                    (file_size_32 == bytes_read)) {
                    // NOTE(aalhendi): file read successfully
                    result.contents_size = file_size_32;
                } else {
                    // TODO(aalhendi): log
                    debug_platform_free_file_memory(thread, result.contents);
                    result.contents = 0;
                }
            } else {
                // TODO(aalhendi): log
            }
        } else {
            // TODO(aalhendi): log
        }

        CloseHandle(file_handle);
    } else {
        // TODO(aalhendi): log
    }

    return (result);
}

internal DEBUG_PLATFORM_WRITE_ENTIRE_FILE(debug_platform_write_entire_file) {
    bool32 result = false;

    HANDLE file_handle = CreateFileA(filename, GENERIC_WRITE, 0, 0, CREATE_ALWAYS, 0, 0);
    if (file_handle != INVALID_HANDLE_VALUE) {
        DWORD bytes_written;
        if (WriteFile(file_handle, memory, memory_size, &bytes_written, 0)) {
            // NOTE(aalhendi): file read successfully
            result = (bytes_written == memory_size);
        } else {
            // TODO(aalhendi): log
        }

        CloseHandle(file_handle);
    } else {
        // TODO(aalhendi): log
    }

    return (result);
}
#endif

internal Win32WindowDimension win32_get_window_dimension(HWND window) {
    RECT client_rect;
    GetClientRect(window, &client_rect);
    Win32WindowDimension result = {
        .height = client_rect.bottom - client_rect.top,
        .width = client_rect.right - client_rect.left,
    };
    return result;
}

internal void win32_display_buffer_in_window(Win32OffscreenBuffer *buff, HDC device_context, i32 window_width,
                                             i32 window_height) {
    // NOTE(aalhendi): for prototyping purposes, we're going to always blit 1-1 pixels to make sure we don't introduce
    // artifacts
    //  with stretching until we get a decent renderer
    if (buff->memory) {
        StretchDIBits(device_context, 0, 0, buff->width, buff->height, 0, 0, buff->width, buff->height, buff->memory,
                      &buff->info, DIB_RGB_COLORS, SRCCOPY);
    }
}

LRESULT CALLBACK win32_main_window_callback(HWND window, u32 msg, WPARAM wparam, LPARAM lparam) {
    LRESULT res = 0;

    switch (msg) {
    case WM_CLOSE: {
        GLOBAL_RUNNING = false;
    } break;
    case WM_DESTROY: {
        GLOBAL_RUNNING = false;
    } break;
    case WM_ACTIVATEAPP: {
#if 1
        if (wparam == TRUE) {
            SetLayeredWindowAttributes(window, RGB(0, 0, 0), 255, LWA_ALPHA);
        } else {
            SetLayeredWindowAttributes(window, RGB(0, 0, 0), 64, LWA_ALPHA);
        }
#endif
    } break;
    case WM_SYSKEYDOWN:
    case WM_SYSKEYUP:
    case WM_KEYDOWN:
    case WM_KEYUP: {
        Assert(!"Keyboard input came in through a non-dispatch message!");
    } break;
    case WM_PAINT: {
        PAINTSTRUCT paint_struct;
        HDC device_context = BeginPaint(window, &paint_struct);
        Win32WindowDimension dimension = win32_get_window_dimension(window);
        win32_display_buffer_in_window(&GLOBAL_BACKBUFFER, device_context, dimension.width, dimension.height);
        EndPaint(window, &paint_struct);
    } break;
    default: {
        res = DefWindowProcA(window, msg, wparam, lparam);
    } break;
    }

    return res;
}

internal bool32 win32_init_d_sound(HWND window_handle, i32 samples_per_second, i32 buffer_size) {
    bool32 result = false;

    // NOTE(aalhendi): load the library
    HMODULE d_sound_lib = LoadLibraryA("dsound.dll");
    if (d_sound_lib) {
        // NOTE(aalhendi): got a dsound object! - cooperative
        DirectSoundCreateFn *direct_sound_create =
            (DirectSoundCreateFn *)GetProcAddress(d_sound_lib, "DirectSoundCreate");

        LPDIRECTSOUND direct_sound;
        if (direct_sound_create && SUCCEEDED(direct_sound_create(0, &direct_sound, 0))) {
            WAVEFORMATEX wave_format = {.wFormatTag = WAVE_FORMAT_PCM,
                                        .nChannels = 2,
                                        .nSamplesPerSec = samples_per_second,
                                        .wBitsPerSample = 16,
                                        .cbSize = 0};
            wave_format.nBlockAlign = (wave_format.nChannels * wave_format.wBitsPerSample) / 8;
            wave_format.nAvgBytesPerSec = wave_format.nSamplesPerSec * wave_format.nBlockAlign;

            if (SUCCEEDED(direct_sound->lpVtbl->SetCooperativeLevel(direct_sound, window_handle, DSSCL_PRIORITY))) {
                // NOTE(aalhendi): we actually can't set the .lpwfxFormat format here, we have to set it later via
                // SetFormat. Windows!
                DSBUFFERDESC buffer_description = {.dwSize = sizeof(buffer_description),
                                                   .dwFlags = DSBCAPS_PRIMARYBUFFER};

                // NOTE(aalhendi): "Create" a primary buffer
                // TODO(aalhendi): DSBCAPS_GLOBALFOCUS
                LPDIRECTSOUNDBUFFER primary_buffer;
                if (SUCCEEDED(direct_sound->lpVtbl->CreateSoundBuffer(direct_sound, &buffer_description,
                                                                      &primary_buffer, 0))) {
                    if (SUCCEEDED(primary_buffer->lpVtbl->SetFormat(primary_buffer, &wave_format))) {
                        // NOTE(aalhendi): we've finally set the format...
                        OutputDebugStringA("Primary buffer format was set.\n");
                    } else {
                        // TODO(aalhendi): handle/log
                    }
                } else {
                    // TODO(aalhendi): handle/log
                }
            } else {
                // TODO(aalhendi): handle/log
            }
            // TODO(aalhendi): DSBCAPS_GETCURRENTPOSITION2
            DSBUFFERDESC buffer_description = {.dwSize = sizeof(buffer_description),
                                               .dwFlags = DSBCAPS_GETCURRENTPOSITION2,
                                               .dwBufferBytes = buffer_size,
                                               .lpwfxFormat = &wave_format};
            if (SUCCEEDED(direct_sound->lpVtbl->CreateSoundBuffer(direct_sound, &buffer_description,
                                                                  &GLOBAL_SECONDARY_BUFFER, 0))) {
                OutputDebugStringA("Secondary buffer created successfully.\n");
                result = true;
            }
        } else {
            // TODO(aalhendi): handle/log
        }
    } else {
        // TODO(aalhendi): handle/log
    }

    return result;
}

internal void win32_clear_buffer(Win32SoundOutput *sound_output) {
    VOID *region1;
    DWORD region1_size;
    VOID *region2;
    DWORD region2_size;
    if (SUCCEEDED(GLOBAL_SECONDARY_BUFFER->lpVtbl->Lock(GLOBAL_SECONDARY_BUFFER, 0, sound_output->buffer_size, &region1,
                                                        &region1_size, &region2, &region2_size, 0))) {
        // TODO(aalhendi): assert that Region1Size/Region2Size is valid
        u8 *dst_sample = (u8 *)region1;
        for (DWORD byte_idx = 0; byte_idx < region1_size; ++byte_idx) {
            *dst_sample++ = 0;
        }

        dst_sample = (u8 *)region2;
        for (DWORD byte_idx = 0; byte_idx < region2_size; ++byte_idx) {
            *dst_sample++ = 0;
        }

        GLOBAL_SECONDARY_BUFFER->lpVtbl->Unlock(GLOBAL_SECONDARY_BUFFER, region1, region1_size, region2, region2_size);
    }
}

internal Win32ReplayBuffer *win32_get_replay_buffer(Win32State *state, int unsigned index) {
    Assert(index < ArrayCount(state->replay_buffers));
    Win32ReplayBuffer *result = &state->replay_buffers[index];
    return (result);
}

internal void win32_begin_recording_input(Win32State *state, int input_recording_idx) {
    Win32ReplayBuffer *replay_buffer = win32_get_replay_buffer(state, input_recording_idx);
    if (replay_buffer->memory) {
        char filename[MAX_PATH];
        win32_get_input_file_location(state, true, input_recording_idx, sizeof(filename), filename);
        state->recording_file_handle = CreateFileA(filename, GENERIC_WRITE, 0, 0, CREATE_ALWAYS, 0, 0);
        if (state->recording_file_handle != INVALID_HANDLE_VALUE) {
            state->input_recording_idx = input_recording_idx;

            // TODO(aalhendi): technically a failure point for 32-bit
            CopyMemory(replay_buffer->memory, state->memory, (size_t)state->total_size);
        }
    }
}

internal void win32_end_recording_input(Win32State *state) {
    if (state->recording_file_handle != INVALID_HANDLE_VALUE) {
        CloseHandle(state->recording_file_handle);
        state->recording_file_handle = INVALID_HANDLE_VALUE;
    }
    state->input_recording_idx = 0;
}

internal void win32_begin_input_playback(Win32State *state, int input_playing_idx) {
    Win32ReplayBuffer *replay_buffer = win32_get_replay_buffer(state, input_playing_idx);
    if (replay_buffer->memory) {
        char filename[MAX_PATH];
        win32_get_input_file_location(state, true, input_playing_idx, sizeof(filename), filename);
        state->playback_file_handle = CreateFileA(filename, GENERIC_READ, 0, 0, OPEN_EXISTING, 0, 0);
        if (state->playback_file_handle != INVALID_HANDLE_VALUE) {
            state->input_playing_idx = input_playing_idx;

            // TODO(aalhendi): technically a failure point for 32-bit
            CopyMemory(state->memory, replay_buffer->memory, (size_t)state->total_size);
        }
    }
}

internal void win32_end_input_playback(Win32State *state) {
    if (state->playback_file_handle != INVALID_HANDLE_VALUE) {
        CloseHandle(state->playback_file_handle);
        state->playback_file_handle = INVALID_HANDLE_VALUE;
    }
    state->input_playing_idx = 0;
}

internal void win32_record_input(Win32State *state, GameInput *new_input) {
    if (state->recording_file_handle != INVALID_HANDLE_VALUE) {
        DWORD bytes_written;
        WriteFile(state->recording_file_handle, new_input, sizeof(*new_input), &bytes_written, 0);
    }
}

internal void win32_playback_input(Win32State *state, GameInput *new_input) {
    if (state->playback_file_handle != INVALID_HANDLE_VALUE) {
        DWORD bytes_read = 0;
        if (ReadFile(state->playback_file_handle, new_input, sizeof(*new_input), &bytes_read, 0)) {
            if (bytes_read == 0) {
                // NOTE(aalhendi): We've hit the end of the stream, go back to the beginning
                int playing_idx = state->input_playing_idx;
                win32_end_input_playback(state);
                win32_begin_input_playback(state, playing_idx);
                if (state->playback_file_handle != INVALID_HANDLE_VALUE) {
                    ReadFile(state->playback_file_handle, new_input, sizeof(*new_input), &bytes_read, 0);
                }
            }
        }
    }
}

internal void win32_fill_sound_buffer(Win32SoundOutput *sound_output, DWORD byte_to_lock, DWORD bytes_to_write,
                                      GameSoundOutputBuffer *source_buffer) {
    // TODO(aalhendi): more test
    VOID *region1;
    DWORD region1_size;
    VOID *region2;
    DWORD region2_size;
    if (SUCCEEDED(GLOBAL_SECONDARY_BUFFER->lpVtbl->Lock(GLOBAL_SECONDARY_BUFFER, byte_to_lock, bytes_to_write, &region1,
                                                        &region1_size, &region2, &region2_size, 0))) {
        // TODO(aalhendi): assert that region sizes are valid

        // TODO(aalhendi):  fn for the loops
        DWORD region1_sample_count = region1_size / sound_output->bytes_per_sample;
        i16 *dst_sample = (i16 *)region1;
        i16 *src_sample = source_buffer->samples;
        for (DWORD sample_idx = 0; sample_idx < region1_sample_count; ++sample_idx) {
            *dst_sample++ = *src_sample++;
            *dst_sample++ = *src_sample++;
            ++sound_output->running_sample_index;
        }

        DWORD region2_sample_count = region2_size / sound_output->bytes_per_sample;
        dst_sample = (i16 *)region2;
        for (DWORD sample_idx = 0; sample_idx < region2_sample_count; ++sample_idx) {
            *dst_sample++ = *src_sample++;
            *dst_sample++ = *src_sample++;
            ++sound_output->running_sample_index;
        }

        GLOBAL_SECONDARY_BUFFER->lpVtbl->Unlock(GLOBAL_SECONDARY_BUFFER, region1, region1_size, region2, region2_size);
    }
}

internal void win32_process_keyboard_message(GameButtonState *new_state, bool32 is_down) {
    if (new_state->ended_down != is_down) {
        new_state->ended_down = is_down;
        ++new_state->half_transition_count;
    }
}

internal void win32_preserve_button_state(GameButtonState *new_buttons, GameButtonState *old_buttons,
                                          int unsigned button_count) {
    for (int unsigned button_idx = 0; button_idx < button_count; ++button_idx) {
        new_buttons[button_idx] = (GameButtonState){0};
        new_buttons[button_idx].ended_down = old_buttons[button_idx].ended_down;
    }
}

internal void win32_process_xinput_digital_button(DWORD xinput_button_state, GameButtonState *old_state,
                                                  DWORD button_bit, GameButtonState *new_state) {
    new_state->ended_down = ((xinput_button_state & button_bit) == button_bit);
    new_state->half_transition_count = (old_state->ended_down != new_state->ended_down) ? 1 : 0;
}

internal f32 win32_process_xinput_stick_value(SHORT value, SHORT dead_zone_threshold) {
    f32 result = 0;

    if (value < -dead_zone_threshold) {
        result = (f32)((value + dead_zone_threshold) / (32768.0f - dead_zone_threshold));
    } else if (value > dead_zone_threshold) {
        result = (f32)((value - dead_zone_threshold) / (32767.0f - dead_zone_threshold));
    }

    return (result);
}

internal void win32_process_pending_messages(Win32State *state, GameControllerInput *keyboard_controller) {
    MSG message;
    while (PeekMessage(&message, 0, 0, 0, PM_REMOVE)) {
        switch (message.message) {
        case WM_QUIT: {
            GLOBAL_RUNNING = false;
        } break;

        case WM_SYSKEYDOWN:
        case WM_SYSKEYUP:
        case WM_KEYDOWN:
        case WM_KEYUP: {
            u32 vk_code = (u32)message.wParam;

            // NOTE(aalhendi): Since we are comparing WasDown to IsDown,
            // we MUST use == and != to convert these bit tests to actual
            // 0 or 1 values.
            bool32 was_down = ((message.lParam & (1 << 30)) != 0);
            bool32 is_down = ((message.lParam & (1 << 31)) == 0);
            if (was_down != is_down) {
                if (vk_code == 'W') {
                    win32_process_keyboard_message(&keyboard_controller->move_up, is_down);
                } else if (vk_code == 'A') {
                    win32_process_keyboard_message(&keyboard_controller->move_left, is_down);
                } else if (vk_code == 'S') {
                    win32_process_keyboard_message(&keyboard_controller->move_down, is_down);
                } else if (vk_code == 'D') {
                    win32_process_keyboard_message(&keyboard_controller->move_right, is_down);
                } else if (vk_code == 'Q') {
                    win32_process_keyboard_message(&keyboard_controller->left_shoulder, is_down);
                } else if (vk_code == 'E') {
                    win32_process_keyboard_message(&keyboard_controller->right_shoulder, is_down);
                } else if (vk_code == VK_UP) {
                    win32_process_keyboard_message(&keyboard_controller->action_up, is_down);
                } else if (vk_code == VK_LEFT) {
                    win32_process_keyboard_message(&keyboard_controller->action_left, is_down);
                } else if (vk_code == VK_DOWN) {
                    win32_process_keyboard_message(&keyboard_controller->action_down, is_down);
                } else if (vk_code == VK_RIGHT) {
                    win32_process_keyboard_message(&keyboard_controller->action_right, is_down);
                } else if (vk_code == VK_ESCAPE) {
                    win32_process_keyboard_message(&keyboard_controller->start, is_down);
                } else if (vk_code == VK_SPACE) {
                    win32_process_keyboard_message(&keyboard_controller->back, is_down);
                }
#if DEBUG_PROFILE
                else if (vk_code == 'P') {
                    if (is_down) {
                        GLOBAL_PAUSE = !GLOBAL_PAUSE;
                    }
                } else if (vk_code == 'L') {
                    if (is_down) {
                        if (state->input_playing_idx == 0) {
                            if (state->input_recording_idx == 0) {
                                win32_begin_recording_input(state, 1);
                            } else {
                                win32_end_recording_input(state);
                                win32_begin_input_playback(state, 1);
                            }
                        } else {
                            win32_end_input_playback(state);
                        }
                    }
                }
#endif
            }

            bool32 alt_key_was_down = (message.lParam & (1 << 29));
            if ((vk_code == VK_F4) && alt_key_was_down) {
                GLOBAL_RUNNING = false;
            }
        } break;

        default: {
            TranslateMessage(&message);
            DispatchMessageA(&message);
        } break;
        }
    }
}

i32 WINAPI WinMain(HINSTANCE h_instance, HINSTANCE h_prev_instance, LPSTR lp_cmd_line, int n_cmd_show) {
    HMODULE module_handle = GetModuleHandleA(NULL);
    Win32State state = {0};
    state.recording_file_handle = INVALID_HANDLE_VALUE;
    state.playback_file_handle = INVALID_HANDLE_VALUE;
    for (i32 i = 0; i < ArrayCount(state.replay_buffers); ++i) {
        state.replay_buffers[i].file_handle = INVALID_HANDLE_VALUE;
        state.replay_buffers[i].memory_map = 0;
    }

    LARGE_INTEGER perf_count_frequency_result;
    QueryPerformanceFrequency(&perf_count_frequency_result); // TODO(aalhendi): fallible
    GLOBAL_PERF_COUNT_FREQUENCY = perf_count_frequency_result.QuadPart;

    win32_get_exe_filename(&state, module_handle);

    char source_dll_name[MAX_PATH];
    char temp_dll_names[2][MAX_PATH];
    win32_build_exe_file_path_name(&state, "handmade.dll", sizeof(source_dll_name), source_dll_name);
    win32_build_exe_file_path_name(&state, "handmade_temp_0.dll", sizeof(temp_dll_names[0]), temp_dll_names[0]);
    win32_build_exe_file_path_name(&state, "handmade_temp_1.dll", sizeof(temp_dll_names[1]), temp_dll_names[1]);

    // NOTE(aalhendi): Set windows scheduler granularity. This is used to make our sleep more accurate (granular).
    u32 desired_scheduler_ms = 1;
    bool32 sleep_is_granular = timeBeginPeriod(desired_scheduler_ms) == TIMERR_NOERROR;
    if (!sleep_is_granular) {
        OutputDebugStringA("Sleep is not granular. This is bad.");
    }

    win32_load_xinput();

    win32_resize_dib_section(&GLOBAL_BACKBUFFER, 960, 540);
    WNDCLASSA wc = {
        .style = CS_HREDRAW | CS_VREDRAW,
        .lpfnWndProc = win32_main_window_callback,
        .hInstance = h_instance,
        .lpszClassName = "HMHWindowClass",
        .hCursor = LoadCursorA(0, IDC_ARROW), // fallible
    };

    if (RegisterClassA(&wc)) {
        // WS_EX_TOPMOST | WS_EX_LAYERED,
        HWND window_handle =
            CreateWindowExA(0, wc.lpszClassName, "Handmade Hero", WS_OVERLAPPEDWINDOW | WS_VISIBLE, CW_USEDEFAULT,
                            CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, NULL, NULL, h_instance, NULL);

        if (window_handle) {
            // TODO(aalhendi): GetSystemMetrics(SM_SAMPLERATE)? How do we reliably query refresh rate? GetComposition?
            i32 monitor_refresh_hz = 60;
            HDC refresh_dc = GetDC(window_handle);
            i32 windows_refresh_rate = GetDeviceCaps(refresh_dc, VREFRESH);
            ReleaseDC(window_handle, refresh_dc);
            // v-refresh rate value of 0 or 1 represents the display hardware's default refresh rate.
            if (windows_refresh_rate > 1) {
                monitor_refresh_hz = windows_refresh_rate;
            }
            f32 game_update_hz = (monitor_refresh_hz / 2.0f);
            f32 target_seconds_per_frame = 1.0f / (f32)game_update_hz;

            // NOTE(aalhendi): audio test
            // TODO(aalhendi): standardize section headings or something
            Win32SoundOutput sound_output = {
                // TODO(aalhendi): make this 60s
                .samples_per_second = 48000,
                .bytes_per_sample = sizeof(i16) * 2,
            };
            // 2 channels, 2 bytes per sample
            sound_output.buffer_size = sound_output.samples_per_second * sound_output.bytes_per_sample;
            // TODO(aalhendi): actually compute this variance and see lowest reasonable value
            sound_output.safety_bytes =
                (i32)(((f32)sound_output.samples_per_second * (f32)sound_output.bytes_per_sample / game_update_hz) /
                      3.0f);
            bool32 sound_is_enabled =
                win32_init_d_sound(window_handle, sound_output.samples_per_second, sound_output.buffer_size);
            if (sound_is_enabled) {
                win32_clear_buffer(&sound_output);
                if (GLOBAL_SECONDARY_BUFFER->lpVtbl->Play(GLOBAL_SECONDARY_BUFFER, 0, 0, DSBPLAY_LOOPING) != DS_OK) {
                    sound_is_enabled = false;
                }
            }

            GLOBAL_RUNNING = true;

#if 0
                        // NOTE(aalhendi): This tests the PlayCursor/WriteCursor update frequency
                        // On the Handmade Hero machine, it was 480 samples.
                        while(GLOBAL_RUNNING)
                        {
                            DWORD play_cursor;
                            DWORD write_cursor;
                            GLOBAL_SECONDARY_BUFFER->lpVtbl->GetCurrentPosition(GLOBAL_SECONDARY_BUFFER, &play_cursor,
                                                                                &write_cursor);

                            char text_buffer[256];
                            _snprintf_s(text_buffer, sizeof(text_buffer),
                                        "PC:%u WC:%u\n", play_cursor, write_cursor);
                            OutputDebugStringA(text_buffer);
                        }
#endif

            // TODO(aalhendi):  pool VirtualAlloc s with bitmap
            i16 *samples = 0;
            if (sound_is_enabled) {
                samples = (i16 *)VirtualAlloc(0, sound_output.buffer_size, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE);
                if (!samples) {
                    sound_is_enabled = false;
                }
            }

#if DEBUG_PROFILE
            LPVOID base_address = (LPVOID)((uintptr_t)Terabytes(2));
#else
            LPVOID base_address = 0;
#endif

            GameMemory game_memory = {
                .permanent_storage_size = Megabytes(64),
                .transient_storage_size = Gigabytes(1),
#if DEBUG_PROFILE
                .debug_platform_free_file_memory = debug_platform_free_file_memory,
                .debug_platform_read_entire_file = debug_platform_read_entire_file,
                .debug_platform_write_entire_file = debug_platform_write_entire_file,
#endif
            };

            // TODO(aalhendi): handle various memory footprints (USING SYSTEM METRICS)
            // TODO(aalhendi): MEM_LARGE_PAGES? adjust token privileges when not on Windows XP?
            state.total_size = game_memory.permanent_storage_size + game_memory.transient_storage_size;
            state.memory =
                VirtualAlloc(base_address, (size_t)state.total_size, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE);
            if (state.memory) {
                game_memory.permanent_storage = state.memory;
                game_memory.transient_storage =
                    ((u8 *)game_memory.permanent_storage + game_memory.permanent_storage_size);
            }

            for (int replay_idx = 0; state.memory && replay_idx < ArrayCount(state.replay_buffers); ++replay_idx) {
                Win32ReplayBuffer *replay_buffer = &state.replay_buffers[replay_idx];

                // TODO(aalhendi): recording system still seems to take too long
                // on record start - find out what Windows is doing and if
                // we can speed up / defer some of that processing.

                win32_get_input_file_location(&state, false, replay_idx, sizeof(replay_buffer->filename),
                                              replay_buffer->filename);

                replay_buffer->file_handle =
                    CreateFileA(replay_buffer->filename, GENERIC_WRITE | GENERIC_READ, 0, 0, CREATE_ALWAYS, 0, 0);
                if (replay_buffer->file_handle != INVALID_HANDLE_VALUE) {
                    LARGE_INTEGER max_size;
                    max_size.QuadPart = state.total_size;
                    replay_buffer->memory_map = CreateFileMapping(replay_buffer->file_handle, 0, PAGE_READWRITE,
                                                                  max_size.HighPart, max_size.LowPart, NULL);

                    if (replay_buffer->memory_map) {
                        replay_buffer->memory =
                            // TODO(aalhendi): technically a failure point for 32-bit
                            MapViewOfFile(replay_buffer->memory_map, FILE_MAP_ALL_ACCESS, 0, 0,
                                          (size_t)state.total_size);
                    }
                }
            }

            if (GLOBAL_BACKBUFFER.memory && game_memory.permanent_storage && game_memory.transient_storage) {
                GameInput input[2] = {0};
                GameInput *new_input = &input[0];
                GameInput *old_input = &input[1];

                LARGE_INTEGER last_counter = win32_get_wall_clock();
                LARGE_INTEGER flip_wall_clock = win32_get_wall_clock();

                int debug_time_marker_idx = 0;
                Win32DebugTimeMarker debug_time_markers[30] = {0};

                DWORD audio_latency_bytes = 0;
                f32 audio_latency_seconds = 0;
                bool32 sound_is_valid = false;

                Win32GameCode game = win32_load_game_code(source_dll_name, temp_dll_names[0]);
                u32 load_counter = 1;

                u64 last_cycle_count = __rdtsc();
                while (GLOBAL_RUNNING) {
                    new_input->dt_for_frame = target_seconds_per_frame;

                    FILETIME new_dll_write_time = win32_get_last_write_time(source_dll_name);
                    if (!game.is_valid || CompareFileTime(&new_dll_write_time, &game.last_write_time) != 0) {
                        Win32GameCode new_game =
                            win32_load_game_code(source_dll_name, temp_dll_names[load_counter++ & 1]);
                        if (new_game.is_valid) {
                            win32_unload_game_code(&game);
                            game = new_game;
                        } else {
                            win32_unload_game_code(&new_game);
                        }
                    }

                    // TODO(aalhendi): zero macro
                    // TODO(aalhendi): can't zero all because the up/down state will be wrong!
                    GameControllerInput *old_keyboard_controller = get_controller(old_input, 0);
                    GameControllerInput *new_keyboard_controller = get_controller(new_input, 0);
                    *new_keyboard_controller = (GameControllerInput){0}; // TODO(aalhendi): zero all struct
                    new_keyboard_controller->is_connected = true;
                    win32_preserve_button_state(new_keyboard_controller->buttons, old_keyboard_controller->buttons,
                                                ArrayCount(new_keyboard_controller->buttons));

                    win32_process_pending_messages(&state, new_keyboard_controller);

                    if (!GLOBAL_PAUSE) {
                        POINT mouse_p;
                        GetCursorPos(&mouse_p);
                        ScreenToClient(window_handle, &mouse_p);
                        new_input->mouse_x = mouse_p.x;
                        new_input->mouse_y = mouse_p.y;
                        new_input->mouse_z = 0; // TODO(aalhendi): mousewheel?
                        win32_preserve_button_state(new_input->mouse_buttons, old_input->mouse_buttons,
                                                    ArrayCount(new_input->mouse_buttons));

                        win32_process_keyboard_message(&new_input->mouse_buttons[0],
                                                       (GetKeyState(VK_LBUTTON) & (1 << 15)) != 0);
                        win32_process_keyboard_message(&new_input->mouse_buttons[1],
                                                       (GetKeyState(VK_MBUTTON) & (1 << 15)) != 0);
                        win32_process_keyboard_message(&new_input->mouse_buttons[2],
                                                       (GetKeyState(VK_RBUTTON) & (1 << 15)) != 0);
                        win32_process_keyboard_message(&new_input->mouse_buttons[3],
                                                       (GetKeyState(VK_XBUTTON1) & (1 << 15)) != 0);
                        win32_process_keyboard_message(&new_input->mouse_buttons[4],
                                                       (GetKeyState(VK_XBUTTON2) & (1 << 15)) != 0);

                        // TODO(aalhendi): dont poll disconnected controllers to avoid xinput frame rate hit on older
                        // libs.
                        // TODO(aalhendi): poll frequency
                        DWORD max_controller_count = XUSER_MAX_COUNT;
                        if (max_controller_count > (ArrayCount(new_input->controllers) - 1)) {
                            max_controller_count = (ArrayCount(new_input->controllers) - 1);
                        }

                        for (DWORD controller_idx = 0; controller_idx < max_controller_count; ++controller_idx) {
                            DWORD our_controller_idx = controller_idx + 1;
                            GameControllerInput *old_controller = get_controller(old_input, our_controller_idx);
                            GameControllerInput *new_controller = get_controller(new_input, our_controller_idx);

                            XINPUT_STATE controller_state;
                            if (XInputGetState(controller_idx, &controller_state) == ERROR_SUCCESS) {
                                new_controller->is_connected = true;
                                new_controller->is_analog = old_controller->is_analog;

                                // NOTE(aalhendi): controller is plugged in
                                // TODO(aalhendi): check controller_state.dwPacketNumber increments too rapidly
                                XINPUT_GAMEPAD *pad = &controller_state.Gamepad;

                                // TODO(aalhendi): square deadzone, check XInput to verify that the deadzone is "round"
                                new_controller->stick_average_x =
                                    win32_process_xinput_stick_value(pad->sThumbLX, XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE);
                                new_controller->stick_average_y =
                                    win32_process_xinput_stick_value(pad->sThumbLY, XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE);
                                if ((new_controller->stick_average_x != 0.0f) ||
                                    (new_controller->stick_average_y != 0.0f)) {
                                    new_controller->is_analog = true;
                                }

                                if (pad->wButtons & XINPUT_GAMEPAD_DPAD_UP) {
                                    new_controller->stick_average_y = 1.0f;
                                    new_controller->is_analog = false;
                                }

                                if (pad->wButtons & XINPUT_GAMEPAD_DPAD_DOWN) {
                                    new_controller->stick_average_y = -1.0f;
                                    new_controller->is_analog = false;
                                }

                                if (pad->wButtons & XINPUT_GAMEPAD_DPAD_LEFT) {
                                    new_controller->stick_average_x = -1.0f;
                                    new_controller->is_analog = false;
                                }

                                if (pad->wButtons & XINPUT_GAMEPAD_DPAD_RIGHT) {
                                    new_controller->stick_average_x = 1.0f;
                                    new_controller->is_analog = false;
                                }

                                f32 threshold = 0.5f;
                                win32_process_xinput_digital_button(
                                    (new_controller->stick_average_x < -threshold) ? 1 : 0, &old_controller->move_left,
                                    1, &new_controller->move_left);
                                win32_process_xinput_digital_button(
                                    (new_controller->stick_average_x > threshold) ? 1 : 0, &old_controller->move_right,
                                    1, &new_controller->move_right);
                                win32_process_xinput_digital_button(
                                    (new_controller->stick_average_y < -threshold) ? 1 : 0, &old_controller->move_down,
                                    1, &new_controller->move_down);
                                win32_process_xinput_digital_button(
                                    (new_controller->stick_average_y > threshold) ? 1 : 0, &old_controller->move_up, 1,
                                    &new_controller->move_up);

                                win32_process_xinput_digital_button(pad->wButtons, &old_controller->action_down,
                                                                    XINPUT_GAMEPAD_A, &new_controller->action_down);
                                win32_process_xinput_digital_button(pad->wButtons, &old_controller->action_right,
                                                                    XINPUT_GAMEPAD_B, &new_controller->action_right);
                                win32_process_xinput_digital_button(pad->wButtons, &old_controller->action_left,
                                                                    XINPUT_GAMEPAD_X, &new_controller->action_left);
                                win32_process_xinput_digital_button(pad->wButtons, &old_controller->action_up,
                                                                    XINPUT_GAMEPAD_Y, &new_controller->action_up);
                                win32_process_xinput_digital_button(pad->wButtons, &old_controller->left_shoulder,
                                                                    XINPUT_GAMEPAD_LEFT_SHOULDER,
                                                                    &new_controller->left_shoulder);
                                win32_process_xinput_digital_button(pad->wButtons, &old_controller->right_shoulder,
                                                                    XINPUT_GAMEPAD_RIGHT_SHOULDER,
                                                                    &new_controller->right_shoulder);

                                win32_process_xinput_digital_button(pad->wButtons, &old_controller->start,
                                                                    XINPUT_GAMEPAD_START, &new_controller->start);
                                win32_process_xinput_digital_button(pad->wButtons, &old_controller->back,
                                                                    XINPUT_GAMEPAD_BACK, &new_controller->back);
                            } else {
                                // NOTE(aalhendi): The controller is not available
                                *new_controller = (GameControllerInput){0};
                            }
                        }

                        ThreadContext thread = {0};

                        GameOffscreenBuffer buffer = {0};
                        buffer.memory = GLOBAL_BACKBUFFER.memory;
                        buffer.width = GLOBAL_BACKBUFFER.width;
                        buffer.height = GLOBAL_BACKBUFFER.height;
                        buffer.pitch = GLOBAL_BACKBUFFER.pitch;
                        buffer.bytes_per_pixel = GLOBAL_BACKBUFFER.bytes_per_pixel;

                        if (state.input_recording_idx) {
                            win32_record_input(&state, new_input);
                        }

                        if (state.input_playing_idx) {
                            win32_playback_input(&state, new_input);
                        }
                        if (game.update_and_render) {
                            game.update_and_render(&thread, &game_memory, new_input, &buffer);
                        }

                        LARGE_INTEGER audio_wall_clock = win32_get_wall_clock();
                        f32 from_begin_to_audio_seconds = win32_get_seconds_elapsed(flip_wall_clock, audio_wall_clock);

                        DWORD play_cursor;
                        DWORD write_cursor;
                        if (sound_is_enabled && GLOBAL_SECONDARY_BUFFER &&
                            GLOBAL_SECONDARY_BUFFER->lpVtbl->GetCurrentPosition(GLOBAL_SECONDARY_BUFFER, &play_cursor,
                                                                                &write_cursor) == DS_OK) {
                            /*
                            NOTE(aalhendi): Here is how sound output computation works.

                            We define a safety value that is the number of samples we think our game update loop
                            may vary by (let's say up to 2 ms)

                            When we wake up to write audio, we will look and see what the play cursor position is,
                            and we will forecast ahead where we think the play cursor will be on the next frame
                            boundary.

                            We will then look to see if the write cursor is before that by our safe amount.
                            If it is, the target fill position is that frame boundary plus one frame.
                            This gives us perfect audio sync in the case of a card that has low enough latency.

                            If the write cursor is *after* that safety margin,
                            then we assume we can never sync the audio perfectly,
                             so we will write one frame's worth of audio plus the safety margin's worth of guard
                            samples.
                             */
                            if (!sound_is_valid) {
                                sound_output.running_sample_index = write_cursor / sound_output.bytes_per_sample;
                                sound_is_valid = true;
                            }

                            DWORD byte_to_lock = ((sound_output.running_sample_index * sound_output.bytes_per_sample) %
                                                  sound_output.buffer_size);

                            DWORD expected_sound_bytes_per_frame =
                                (int)((f32)(sound_output.samples_per_second * sound_output.bytes_per_sample) /
                                      game_update_hz);
                            f32 seconds_left_until_flip = (target_seconds_per_frame - from_begin_to_audio_seconds);
                            if (seconds_left_until_flip < 0.0f) {
                                seconds_left_until_flip = 0.0f;
                            }
                            DWORD expected_bytes_until_flip =
                                (DWORD)((seconds_left_until_flip / target_seconds_per_frame) *
                                        (f32)expected_sound_bytes_per_frame);

                            DWORD expected_frame_boundary_byte = play_cursor + expected_bytes_until_flip;

                            DWORD safe_write_cursor = write_cursor;
                            if (safe_write_cursor < play_cursor) {
                                safe_write_cursor += sound_output.buffer_size;
                            }
                            Assert(safe_write_cursor >= play_cursor);
                            safe_write_cursor += sound_output.safety_bytes;

                            bool32 audio_card_is_low_latency = (safe_write_cursor < expected_frame_boundary_byte);

                            DWORD target_cursor = 0;
                            if (audio_card_is_low_latency) {
                                target_cursor = (expected_frame_boundary_byte + expected_sound_bytes_per_frame);
                            } else {
                                target_cursor =
                                    (write_cursor + expected_sound_bytes_per_frame + sound_output.safety_bytes);
                            }
                            target_cursor = (target_cursor % sound_output.buffer_size);

                            DWORD bytes_to_write = 0;
                            if (byte_to_lock > target_cursor) {
                                bytes_to_write = (sound_output.buffer_size - byte_to_lock);
                                bytes_to_write += target_cursor;
                            } else {
                                bytes_to_write = target_cursor - byte_to_lock;
                            }

                            GameSoundOutputBuffer sound_buffer = {0};
                            sound_buffer.samples_per_second = sound_output.samples_per_second;
                            sound_buffer.sample_count = bytes_to_write / sound_output.bytes_per_sample;
                            sound_buffer.samples = samples;
                            if (game.get_sound_samples) {
                                game.get_sound_samples(&thread, &game_memory, &sound_buffer);
                            }

#if DEBUG_PROFILE
                            Win32DebugTimeMarker *marker = &debug_time_markers[debug_time_marker_idx];
                            marker->output_play_cursor = play_cursor;
                            marker->output_write_cursor = write_cursor;
                            marker->output_location = byte_to_lock;
                            marker->output_byte_count = bytes_to_write;
                            marker->expected_flip_play_cursor = expected_frame_boundary_byte;

                            DWORD unwrapped_write_cursor = write_cursor;
                            if (unwrapped_write_cursor < play_cursor) {
                                unwrapped_write_cursor += sound_output.buffer_size;
                            }
                            audio_latency_bytes = unwrapped_write_cursor - play_cursor;
                            audio_latency_seconds = (((f32)audio_latency_bytes / (f32)sound_output.bytes_per_sample) /
                                                     (f32)sound_output.samples_per_second);

#if 0
                                        char text_buffer[256];
                                        _snprintf_s(text_buffer, sizeof(text_buffer),
                                                    "BTL:%u TC:%u BTW:%u - PC:%u WC:%u DELTA:%u (%fs)\n",
                                                    byte_to_lock, target_cursor, bytes_to_write,
                                                    play_cursor, write_cursor, audio_latency_bytes, audio_latency_seconds);
                                        OutputDebugStringA(text_buffer);
#endif
#endif
                            // NOTE(aalhendi): ideally, we want to only fill sound buffer if there's something to write.
                            // this fn calls Lock(), which can fail if bytes_to_write is 0,
                            // we are ignoring the error for now, skipping this frame if it fails. This is to match C
                            // behavior. from testing, this only happens the first time the call occurs. we could have
                            // an if check to see if bytes_to_write is 0, but that check would run every frame, which is
                            // not ideal.
                            win32_fill_sound_buffer(&sound_output, byte_to_lock, bytes_to_write, &sound_buffer);
                        } else {
                            sound_is_valid = false;
                        }

                        LARGE_INTEGER work_counter = win32_get_wall_clock();
                        f32 work_seconds_elapsed = win32_get_seconds_elapsed(last_counter, work_counter);

                        // TODO(aalhendi): NOT TESTED YET!
                        f32 seconds_elapsed_for_frame = work_seconds_elapsed;
                        if (seconds_elapsed_for_frame < target_seconds_per_frame) {
                            if (sleep_is_granular) {
                                DWORD sleep_ms =
                                    (DWORD)(1000.0f * (target_seconds_per_frame - seconds_elapsed_for_frame));
                                if (sleep_ms > 0) {
                                    Sleep(sleep_ms);
                                }
                            }

                            f32 test_seconds_elapsed_for_frame =
                                win32_get_seconds_elapsed(last_counter, win32_get_wall_clock());
                            if (test_seconds_elapsed_for_frame < target_seconds_per_frame) {
                                // TODO(aalhendi): LOG MISSED SLEEP HERE
                            }

                            while (seconds_elapsed_for_frame < target_seconds_per_frame) {
                                seconds_elapsed_for_frame =
                                    win32_get_seconds_elapsed(last_counter, win32_get_wall_clock());
                            }
                        } else {
                            // TODO(aalhendi): missed frame. log as well
                        }

                        LARGE_INTEGER end_counter = win32_get_wall_clock();
                        f32 ms_per_frame = 1000.0f * win32_get_seconds_elapsed(last_counter, end_counter);
                        last_counter = end_counter;

                        Win32WindowDimension dimension = win32_get_window_dimension(window_handle);
                        HDC device_context = GetDC(window_handle);
                        win32_display_buffer_in_window(&GLOBAL_BACKBUFFER, device_context, dimension.width,
                                                       dimension.height);
                        ReleaseDC(window_handle, device_context);

                        flip_wall_clock = win32_get_wall_clock();
#if DEBUG_PROFILE
                        // NOTE(aalhendi): debug code
                        {
                            DWORD flip_play_cursor;
                            DWORD flip_write_cursor;
                            if (sound_is_enabled && GLOBAL_SECONDARY_BUFFER &&
                                GLOBAL_SECONDARY_BUFFER->lpVtbl->GetCurrentPosition(
                                    GLOBAL_SECONDARY_BUFFER, &flip_play_cursor, &flip_write_cursor) == DS_OK) {
                                Assert(debug_time_marker_idx < ArrayCount(debug_time_markers));
                                Win32DebugTimeMarker *marker = &debug_time_markers[debug_time_marker_idx];
                                marker->flip_play_cursor = flip_play_cursor;
                                marker->flip_write_cursor = flip_write_cursor;
                            }
                        }
#endif

                        GameInput *temp = new_input;
                        new_input = old_input;
                        old_input = temp;
                        // TODO(aalhendi): clear these here?

#if 0
                                    u64 end_cycle_count = __rdtsc();
                                    u64 cycles_elapsed = end_cycle_count - last_cycle_count;
                                    last_cycle_count = end_cycle_count;

                                    f64 fps = 0.0f;
                                    f64 mcpf = ((f64)cycles_elapsed / (1000.0f * 1000.0f));

                                    char fps_buffer[256];
                                    _snprintf_s(fps_buffer, sizeof(fps_buffer),
                                                "%.02fms/f,  %.02ff/s,  %.02fmc/f\n", ms_per_frame, fps, mcpf);
                                    OutputDebugStringA(fps_buffer);
#endif

#if DEBUG_PROFILE
                        ++debug_time_marker_idx;
                        if (debug_time_marker_idx == ArrayCount(debug_time_markers)) {
                            debug_time_marker_idx = 0;
                        }
#endif
                    }
                }
            } else {
                // TODO(aalhendi): log
            }

        } else {
            // TODO(aalhendi): log/handle
        }
    } else {
        // TODO(aalhendi): log/handle
    }

    return 0;
}
