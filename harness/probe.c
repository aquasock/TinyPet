#include <windows.h>
#include <stdio.h>

typedef void (*fn_void)(void);

int main(void)
{
    HMODULE h = LoadLibraryA("Petz 4.exe");
    printf("LoadLibraryA(Petz 4.exe) -> %p err=%lu\n", (void*)h, GetLastError());
    if (!h) return 1;

    fn_void init_math = (fn_void)GetProcAddress(h, "?InitMath@@YAXXZ");
    printf("InitMath -> %p\n", (void*)init_math);
    if (init_math) {
        printf("calling InitMath()...\n"); fflush(stdout);
        init_math();
        printf("InitMath() returned\n"); fflush(stdout);
    }

    const char *probe[] = {
        "?XFillRotatedCircle@XDrawPort@@QAE_NPBVCircleRenderBlock@@@Z",
        "?SetBallTextureInfo@XBallz@@QAEXAAVCircleRenderBlock@@ABVBallState@@ABV?$pfvector@VXPointRot3_16@@PBD@@H@Z",
        "?Init@XDib@@QAEXPAXW4ColorTypes@@@Z",
        "?InitBallz@XBallz@@QAEXPBDPBVLinez@@0@Z",
    };
    for (int i = 0; i < 4; i++) {
        FARPROC p = GetProcAddress(h, probe[i]);
        printf("  %-28.28s -> %p\n", probe[i] + 1, (void*)p);
    }
    return 0;
}
