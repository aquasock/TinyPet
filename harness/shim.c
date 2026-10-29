#include <windows.h>

static void logline(const char *msg)
{
    HANDLE f = CreateFileA("petzshim.log", FILE_APPEND_DATA, FILE_SHARE_READ,
                           NULL, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (f == INVALID_HANDLE_VALUE) return;
    DWORD n;
    SetFilePointer(f, 0, NULL, FILE_END);
    WriteFile(f, msg, (DWORD)lstrlenA(msg), &n, NULL);
    WriteFile(f, "\r\n", 2, &n, NULL);
    CloseHandle(f);
}

BOOL WINAPI DllMain(HINSTANCE hinst, DWORD reason, LPVOID reserved)
{
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        char buf[160];
        wsprintfA(buf, "attach pid=%lu hinst=%p", GetCurrentProcessId(), (void*)hinst);
        logline(buf);
        DisableThreadLibraryCalls(hinst);
    }
    return TRUE;
}
