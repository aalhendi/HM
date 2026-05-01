#include <windows.h>

int WINAPI WinMain(HINSTANCE hInstance, HINSTANCE hPrevInstance, LPSTR lpCmdLine, int nCmdShow)
{
    OutputDebugStringA("Hello, World!");
    MessageBoxA(NULL, "Hello, World!", "Hello", MB_OK);
    return 0;
}
