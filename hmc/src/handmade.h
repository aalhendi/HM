#if !defined(HANDMADE_H)
/*
  NOTE(aalhendi): poor man's feature flags

  DEBUG_PROFILE:
    0 - prod
    1 - debug

  DEBUG_ASSERTIONS:
    0 - slow?
    1 - fast!
*/

#include <stdint.h>

typedef int8_t i8;
typedef int16_t i16;
typedef int32_t i32;
typedef int64_t i64;

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;

typedef float f32;
typedef double f64;

typedef i32 bool32;

#define global_variable static
#define local_persist static
#define internal static

#define true 1
#define false 0

// -----------------------------------------------------------------------------
// ASSERTIONS & UNREACHABLE LOGIC
// -----------------------------------------------------------------------------

#if DEBUG_ASSERTIONS

#if defined(_MSC_VER)
#define DEBUG_BREAK() __debugbreak()
#elif defined(__clang__) || defined(__GNUC__)
#define DEBUG_BREAK() __builtin_trap()
#else
// NOTE(aalhendi): fallback if not using MSVC/Clang/GCC, force crash by writing to null
#define DEBUG_BREAK() (*(volatile int *)0 = 0)
#endif

#define Assert(expression)                                                                                             \
    do {                                                                                                               \
        if (!(expression)) {                                                                                           \
            DEBUG_BREAK();                                                                                             \
        }                                                                                                              \
    } while (0)

#define UNREACHABLE()                                                                                                  \
    do {                                                                                                               \
        Assert(0 && "Unreachable code executed!");                                                                     \
    } while (0)

#else

#define Assert(expression)                                                                                             \
    do {                                                                                                               \
    } while (0)

// NOTE(aalhendi): ask LLVM/GCC/MSVC to aggressively optimize away branches
#if defined(__GNUC__) || defined(__clang__)
#define UNREACHABLE() __builtin_unreachable()
#elif defined(_MSC_VER)
#define UNREACHABLE() __assume(0)
#else
#define UNREACHABLE()
#endif

#endif // DEBUG_ASSERTIONS

#define Kilobytes(value) ((u64)(value) * 1024ULL)
#define Megabytes(value) (Kilobytes(value) * 1024ULL)
#define Gigabytes(value) (Megabytes(value) * 1024ULL)
#define Terabytes(value) (Gigabytes(value) * 1024ULL)

#define ArrayCount(arr) (sizeof(arr) / sizeof((arr)[0]))
// TODO(aalhendi): other array ops. swap, min, max macro/fn?

inline u32 truncate_u64_to_u32_safe(u64 value) {
    // TODO(aalhendi): Defines for u64_MAX etc
    Assert(value <= 0xFFFFFFFF);
    u32 res = (u32)value;
    return res;
}

typedef struct ThreadContext {
    int placeholder;
} ThreadContext;

// NOTE(aalhendi): services that platform layer provides to game
#if DEBUG_PROFILE
/* IMPORTANT(aalhendi):
   These are NOT for doing anything in prod build.
   writes are blocking doesn't protect against lost data
*/
typedef struct DebugFileReadResult {
    u32 contents_size;
    void *contents;
} DebugFileReadResult;

#define DEBUG_PLATFORM_FREE_FILE_MEMORY(name) void name(ThreadContext *thread, void *memory)
typedef DEBUG_PLATFORM_FREE_FILE_MEMORY(DebugPlatformFreeFileMemoryFn);

#define DEBUG_PLATFORM_READ_ENTIRE_FILE(name) DebugFileReadResult name(ThreadContext *thread, char *filename)
typedef DEBUG_PLATFORM_READ_ENTIRE_FILE(DebugPlatformReadEntireFileFn);

#define DEBUG_PLATFORM_WRITE_ENTIRE_FILE(name)                                                                         \
    bool32 name(ThreadContext *thread, char *filename, u32 memory_size, void *memory)
typedef DEBUG_PLATFORM_WRITE_ENTIRE_FILE(DebugPlatformWriteEntireFileFn);

#endif // DEBUG_PROFILE

/*
  NOTE(aalhendi): Services that the game provides to the platform layer.
*/

// FOUR THINGS - timing, controller/keyboard input, bitmap buffer to use, sound
// buffer to use

// TODO(aalhendi): Rendering specifically will be a three-tiered abstraction.
typedef struct GameOffscreenBuffer {
    // NOTE(aalhendi): pixels always 32-bit, BB GG RR XX
    void *memory;
    int width;
    int height;
    int pitch;
    int bytes_per_pixel;
} GameOffscreenBuffer;

typedef struct GameSoundOutputBuffer {
    int samples_per_second;
    int sample_count;
    i16 *samples;
} GameSoundOutputBuffer;

typedef struct GameButtonState {
    int half_transition_count;
    bool32 ended_down;
} GameButtonState;

typedef struct GameControllerInput {
    bool32 is_connected;
    bool32 is_analog;
    f32 stick_average_x;
    f32 stick_average_y;

    union {
        GameButtonState buttons[12];
        struct {
            GameButtonState move_up;
            GameButtonState move_down;
            GameButtonState move_left;
            GameButtonState move_right;

            GameButtonState action_up;
            GameButtonState action_down;
            GameButtonState action_left;
            GameButtonState action_right;

            GameButtonState left_shoulder;
            GameButtonState right_shoulder;

            GameButtonState back;
            GameButtonState start;

            // NOTE(aalhendi): all buttons must be added above this line

            GameButtonState terminator;
        };
    };
} GameControllerInput;

typedef struct GameInput {
    GameButtonState mouse_buttons[5];
    i32 mouse_x, mouse_y, mouse_z;

    f32 dt_for_frame;

    GameControllerInput controllers[5];
} GameInput;
inline GameControllerInput *get_controller(GameInput *input, int unsigned controller_idx) {
    Assert(controller_idx < ArrayCount(input->controllers));

    GameControllerInput *result = &input->controllers[controller_idx];
    return (result);
}

typedef struct GameMemory {
    bool32 is_initialized;

    u64 permanent_storage_size;
    void *permanent_storage; // NOTE(aalhendi): MUST be zeroed at startup

    u64 transient_storage_size;
    void *transient_storage; // NOTE(aalhendi): MUST be zeroed at startup

#if DEBUG_PROFILE
    DebugPlatformFreeFileMemoryFn *debug_platform_free_file_memory;
    DebugPlatformReadEntireFileFn *debug_platform_read_entire_file;
    DebugPlatformWriteEntireFileFn *debug_platform_write_entire_file;
#endif
} GameMemory;

#define GAME_UPDATE_AND_RENDER(name)                                                                                   \
    void name(ThreadContext *thread, GameMemory *memory, GameInput *input, GameOffscreenBuffer *buff)
typedef GAME_UPDATE_AND_RENDER(GameUpdateAndRender);

// NOTE(aalhendi): has to be a very fast function, <1ms ~
// TODO(aalhendi): reduce the pressure on this fn perf by profiling
#define GAME_GET_SOUND_SAMPLES(name)                                                                                   \
    void name(ThreadContext *thread, GameMemory *memory, GameSoundOutputBuffer *sound_buffer)
typedef GAME_GET_SOUND_SAMPLES(GameGetSoundSamples);

//
//
//

typedef struct GameState {
    f32 player_x;
    f32 player_y;
} GameState;

#define HANDMADE_H
#endif
