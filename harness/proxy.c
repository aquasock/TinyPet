/*
 * petzshim — proxy for UVocab.dll, loaded into the retail Petz 4 process.
 *
 * The retail Petz 4.exe imports exactly one symbol from UVocab.dll
 * (VtGeneratePronunciations). We forward that to the renamed original and use
 * DllMain to install hooks in the main module's import table:
 *
 *   time        timeGetTime / GetTickCount / QueryPerformanceCounter are driven
 *               by a frame counter instead of the wall clock. Time moves only
 *               when the game presents a frame, so two runs advance together.
 *
 *   framebuffer CreateDIBSection is hooked to learn each drawing surface's
 *               address, size and format. BitBlt is hooked as the "present":
 *               it bumps the frame counter, hashes every surface whose geometry
 *               matches the blit, and dumps them verbatim once the scene is up.
 *
 * The game does not use SetDIBitsToDevice/StretchDIBits; it draws into a DIB
 * section and BitBlts it. Hooking the DIB gives us pixels before any window
 * manager or Wine composition touches them.
 *
 * Log: petzshim.log in the game's working directory.
 */
#include <windows.h>
#include <string.h>

/* ---------------------------------------------------------------- logging */

static void slog(const char *msg)
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

static char *put_uint(char *p, unsigned long v)
{
    char tmp[16];
    int i = 0;
    if (!v) { *p++ = '0'; return p; }
    while (v) { tmp[i++] = (char)('0' + (v % 10)); v /= 10; }
    while (i) *p++ = tmp[--i];
    return p;
}

static char *put_hex64(char *p, unsigned long long v)
{
    for (int shift = 60; shift >= 0; shift -= 4) {
        int d = (int)((v >> shift) & 0xF);
        *p++ = (char)(d < 10 ? '0' + d : 'a' + d - 10);
    }
    return p;
}

/* ------------------------------------------------------------ clock hooks */

#define FRAME_US 16667
#define QPC_FREQ 10000000ULL

/* Time advances 1 ms per clock query. Deterministic because the game's call
 * sequence is a property of its code path, and it cannot deadlock on a frozen
 * clock the way a per-frame clock would. */
static volatile LONG g_clock_calls = 0;
static volatile LONG g_presents = 0;

static DWORD WINAPI hook_timeGetTime(void)
{
    return (DWORD)InterlockedIncrement(&g_clock_calls);
}

static DWORD WINAPI hook_GetTickCount(void)
{
    return (DWORD)InterlockedIncrement(&g_clock_calls);
}

static BOOL WINAPI hook_QueryPerformanceCounter(LARGE_INTEGER *out)
{
    LONG c = InterlockedIncrement(&g_clock_calls);
    if (out) out->QuadPart = (LONGLONG)c * (LONGLONG)(QPC_FREQ / 1000);
    return TRUE;
}

static BOOL WINAPI hook_QueryPerformanceFrequency(LARGE_INTEGER *out)
{
    if (out) out->QuadPart = (LONGLONG)QPC_FREQ;
    return TRUE;
}

/* ------------------------------------------------------------- framebuffer */

#define MAX_SURFACES 8

typedef struct {
    void *bits;
    int   width, height, bpp, stride;
    int   has_pal;
    unsigned char pal[256 * 4];   /* RGBQUAD copy for 8bpp surfaces */
} Surface;

static Surface g_surf[MAX_SURFACES];
static int     g_surf_count = 0;
static int     g_dumps = 0;
#define DUMP_LIMIT 40
static volatile LONG g_next_dump = 1000;

typedef int (WINAPI *pBitBlt)(HDC, int, int, int, int, HDC, int, int, DWORD);
typedef int (WINAPI *pStretchBlt)(HDC,int,int,int,int,HDC,int,int,int,int,DWORD);
typedef HANDLE (WINAPI *pCreateDIBSection)(HDC, const BITMAPINFO *, UINT,
                                           void **, HANDLE, DWORD);

static pBitBlt real_BitBlt = NULL;
static pStretchBlt real_StretchBlt = NULL;
static pCreateDIBSection real_CreateDIBSection = NULL;

static unsigned long long fnv1a(const unsigned char *d, size_t len)
{
    unsigned long long h = 1469598103934665603ULL;
    for (size_t i = 0; i < len; i++) { h ^= d[i]; h *= 1099511628211ULL; }
    return h;
}

static void dump_surface(const Surface *s, long frame)
{
    char name[80], *q = name;
    lstrcpyA(q, "dib-f"); q += 5;
    q = put_uint(q, (unsigned long)frame);
    *q++ = '-';
    q = put_uint(q, (unsigned long)s->bpp);
    lstrcpyA(q, "bpp.raw"); q += 8;
    *q = 0;

    HANDLE f = CreateFileA(name, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS,
                           FILE_ATTRIBUTE_NORMAL, NULL);
    if (f == INVALID_HANDLE_VALUE) return;
    DWORD n, hdr[6] = { (DWORD)s->width, (DWORD)s->height, (DWORD)s->bpp,
                        (DWORD)s->stride, (DWORD)frame, (DWORD)s->has_pal };
    WriteFile(f, hdr, sizeof(hdr), &n, NULL);
    if (s->has_pal) WriteFile(f, s->pal, 256 * 4, &n, NULL);
    DWORD want = (DWORD)(s->stride * s->height);
    BOOL ok = WriteFile(f, s->bits, want, &n, NULL);
    if (!ok || n != want) {
        char msg[128], *m = msg;
        lstrcpyA(m, "DUMP FAILED wrote="); m += 18;
        m = put_uint(m, (unsigned long)n);
        lstrcpyA(m, " want="); m += 6;
        m = put_uint(m, (unsigned long)want);
        *m = 0;
        slog(msg);
    }
    CloseHandle(f);
}

static void note_present(const char *api, int cx, int cy)
{
    LONG frame = InterlockedIncrement(&g_presents) - 1;
    LONG clk = g_clock_calls;

    char buf[320], *p = buf;
    lstrcpyA(p, "present "); p += lstrlenA(p);
    lstrcpyA(p, api); p += lstrlenA(api);
    *p++ = ' ';
    p = put_uint(p, (unsigned long)frame);
    *p++ = ' ';
    p = put_uint(p, (unsigned long)cx);
    *p++ = 'x';
    p = put_uint(p, (unsigned long)cy);

    for (int i = 0; i < g_surf_count; i++) {
        Surface *s = &g_surf[i];
        if (!s->bits || s->width != cx || s->height != cy) continue;
        lstrcpyA(p, " |"); p += lstrlenA(p);
        p = put_uint(p, (unsigned long)s->bpp);
        lstrcpyA(p, "bpp="); p += lstrlenA(p);
        p = put_hex64(p, fnv1a((const unsigned char *)s->bits,
                               (size_t)s->stride * s->height));
    }
    *p = 0;
    if (frame < 3) slog(buf);

    /* Dump the biggest 8bpp surface every N presents. Keyed on drawn frames,
     * not the clock: the title screen can stop querying time entirely, which
     * starves a clock-based trigger. */
    if (frame >= g_next_dump && g_dumps < DUMP_LIMIT) {
        g_next_dump = frame + 1000;
        /* Prefer the surface that matches the screen: that is the scene
         * buffer. Falling back to "largest" picked a 1024x768 canvas the game
         * had already deleted, and the write silently produced 0 bytes. */
        int sw = GetSystemMetrics(SM_CXSCREEN);
        int sh = GetSystemMetrics(SM_CYSCREEN);
        Surface *best = NULL;
        for (int i = 0; i < g_surf_count; i++) {
            Surface *s = &g_surf[i];
            if (!s->bits || s->bpp != 8) continue;
            if (s->width == sw && s->height == sh) { best = s; break; }
            if (!best || (long)s->width * s->height > (long)best->width * best->height)
                best = s;
        }
        if (best) { dump_surface(best, frame); g_dumps++; }
    }
}

static int WINAPI hook_BitBlt(HDC hdcDst, int x, int y, int cx, int cy,
                              HDC hdcSrc, int x1, int y1, DWORD rop)
{
    note_present("BitBlt", cx, cy);
    if (real_BitBlt) return real_BitBlt(hdcDst, x, y, cx, cy, hdcSrc, x1, y1, rop);
    return 1;
}

static int WINAPI hook_StretchBlt(HDC hdcDst, int x, int y, int cx, int cy,
                                  HDC hdcSrc, int x1, int y1, int sx, int sy, DWORD rop)
{
    note_present("StretchBlt", cx, cy);
    if (real_StretchBlt) return real_StretchBlt(hdcDst, x, y, cx, cy, hdcSrc, x1, y1, sx, sy, rop);
    return 1;
}

static HANDLE WINAPI hook_CreateDIBSection(HDC hdc, const BITMAPINFO *bmi, UINT usage,
                                           void **ppvBits, HANDLE hSection, DWORD offset)
{
    HANDLE r = real_CreateDIBSection
        ? real_CreateDIBSection(hdc, bmi, usage, ppvBits, hSection, offset)
        : NULL;

    if (r && bmi && ppvBits && *ppvBits && g_surf_count < MAX_SURFACES) {
        int w = bmi->bmiHeader.biWidth;
        int h = bmi->bmiHeader.biHeight;
        if (h < 0) h = -h;
        int bpp = bmi->bmiHeader.biBitCount;
        Surface *s = &g_surf[g_surf_count++];
        s->bits = *ppvBits;
        s->width = w;
        s->height = h;
        s->bpp = bpp;
        s->stride = ((w * bpp + 31) / 32) * 4;
        s->has_pal = 0;
        if (bpp == 8) {
            memcpy(s->pal, bmi->bmiColors, 256 * 4);
            s->has_pal = 1;
        }

        char buf[160], *p = buf;
        lstrcpyA(p, "DIB "); p += lstrlenA(p);
        p = put_uint(p, (unsigned long)w);
        *p++ = 'x';
        p = put_uint(p, (unsigned long)h);
        *p++ = ' ';
        p = put_uint(p, (unsigned long)bpp);
        lstrcpyA(p, "bpp stride="); p += lstrlenA(p);
        p = put_uint(p, (unsigned long)s->stride);
        *p = 0;
        slog(buf);
    }
    return r;
}

/* ------------------------------------------------------------- palette */

typedef HPALETTE (WINAPI *pCreatePalette)(const LOGPALETTE *);

static pCreatePalette real_CreatePalette = NULL;

static void dump_palette(const LOGPALETTE *lp)
{
    if (!lp) return;
    WORD count = lp->palNumEntries;
    HANDLE f = CreateFileA("palette.raw", GENERIC_WRITE, 0, NULL, CREATE_ALWAYS,
                           FILE_ATTRIBUTE_NORMAL, NULL);
    if (f == INVALID_HANDLE_VALUE) return;
    DWORD n;
    WORD hdr[2] = { lp->palVersion, count };
    WriteFile(f, hdr, sizeof(hdr), &n, NULL);
    WriteFile(f, lp->palPalEntry, (DWORD)(count * sizeof(PALETTEENTRY)), &n, NULL);
    CloseHandle(f);

    char buf[64], *p = buf;
    lstrcpyA(p, "palette entries="); p += lstrlenA(p);
    p = put_uint(p, (unsigned long)count);
    *p = 0;
    slog(buf);
}

static HPALETTE WINAPI hook_CreatePalette(const LOGPALETTE *lp)
{
    dump_palette(lp);
    return real_CreatePalette ? real_CreatePalette(lp) : NULL;
}

/* ------------------------------------------------------- inline hook */

extern void fill_entry(void);
void *fill_trampoline = NULL;

static volatile LONG g_fill_calls = 0;
static unsigned char *g_prev[MAX_SURFACES];
static int g_have_prev = 0;
static int g_written = 0;
static int g_seq = 0;
#define MAX_SEQ 6
static unsigned char g_block[512];
static int g_block_len = 0;

static void snapshot_surface(int i, Surface *s)
{
    if (!s->bits) return;
    size_t bytes = (size_t)s->stride * s->height;
    if (!g_prev[i])
        g_prev[i] = (unsigned char *)VirtualAlloc(NULL, bytes,
                        MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (g_prev[i]) memcpy(g_prev[i], s->bits, bytes);
}

/* Returns changed-pixel count and fills the bounding box. */
static long diff_surface(int i, Surface *s, int *minx, int *miny, int *maxx, int *maxy)
{
    if (!s->bits || !g_prev[i]) return 0;
    long changed = 0;
    *minx = s->width; *miny = s->height; *maxx = -1; *maxy = -1;
    for (int y = 0; y < s->height; y++) {
        const unsigned char *a = g_prev[i] + (size_t)y * s->stride;
        const unsigned char *b = (const unsigned char *)s->bits + (size_t)y * s->stride;
        for (int x = 0; x < s->width; x++) {
            if (a[x] != b[x]) {
                changed++;
                if (x < *minx) *minx = x;
                if (x > *maxx) *maxx = x;
                if (y < *miny) *miny = y;
                if (y > *maxy) *maxy = y;
            }
        }
    }
    return changed;
}

/* Dump the static circle lookup tables built at startup by
 * _initStaticCircleLookup. XFillRotatedCircle indexes them with triangular
 * numbers, so having the real table beats guessing its layout. */
static void dump_tables(void)
{
    static const unsigned long TAB_RVA = 0x23b458;   /* VA 0x63b458 at base 0x400000 */
    const unsigned char *base = (const unsigned char *)GetModuleHandleA(NULL);
    const unsigned char *tab = base + TAB_RVA;
    HANDLE f = CreateFileA("tables.raw", GENERIC_WRITE, 0, NULL, CREATE_ALWAYS,
                           FILE_ATTRIBUTE_NORMAL, NULL);
    if (f == INVALID_HANDLE_VALUE) return;
    DWORD n;
    WriteFile(f, tab, 0x8000, &n, NULL);
    CloseHandle(f);
    char buf[96], *q = buf;
    lstrcpyA(q, "tables dumped at "); q += lstrlenA(q);
    q = put_hex64(q, (unsigned long long)(ULONG_PTR)tab);
    *q = 0;
    slog(buf);
}

void fill_probe(void *drawport, void *block)
{
    LONG n = InterlockedIncrement(&g_fill_calls);
    if (!block) return;

    {
        char b2[96], *q = b2;
        lstrcpyA(q, "fill call "); q += lstrlenA(q);
        q = put_uint(q, (unsigned long)n);
        *q = 0; slog(b2);
    }

    if (n == 1) dump_tables();

    if (!g_have_prev) {
        for (int i = 0; i < g_surf_count; i++) snapshot_surface(i, &g_surf[i]);
        memcpy(g_block, block, sizeof(g_block));
        g_block_len = (int)sizeof(g_block);
        g_have_prev = 1;
        return;
    }

    /* Compare against the previous fill's snapshot: whatever moved is what that
     * fill drew (fills are sparse, so contamination is minimal). */
    if (g_seq < MAX_SEQ) {
        int best = -1; long best_changed = 0;
        int bminx = 0, bminy = 0, bmaxx = 0, bmaxy = 0;
        for (int i = 0; i < g_surf_count; i++) {
            int minx, miny, maxx, maxy;
            long ch = diff_surface(i, &g_surf[i], &minx, &miny, &maxx, &maxy);
            if (ch > best_changed) {
                best_changed = ch; best = i;
                bminx = minx; bminy = miny; bmaxx = maxx; bmaxy = maxy;
            }
        }
        if (best >= 0 && best_changed > 0) {
            Surface *s = &g_surf[best];
            size_t bytes = (size_t)s->stride * s->height;
            char fname[64], *fp = fname;
            lstrcpyA(fp, "ball-input-"); fp += lstrlenA(fp);
            fp = put_uint(fp, (unsigned long)g_seq);
            lstrcpyA(fp, ".raw"); fp += lstrlenA(fp);
            *fp = 0;
            HANDLE f = CreateFileA(fname, GENERIC_WRITE, 0, NULL,
                                   CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
            if (f != INVALID_HANDLE_VALUE) {
                DWORD w, hdr[9] = {
                    (DWORD)s->width, (DWORD)s->height, (DWORD)s->bpp,
                    (DWORD)s->stride, (DWORD)bminx, (DWORD)bminy,
                    (DWORD)(bmaxx - bminx + 1), (DWORD)(bmaxy - bminy + 1),
                    (DWORD)best_changed
                };
                WriteFile(f, hdr, sizeof(hdr), &w, NULL);
                if (s->has_pal) WriteFile(f, s->pal, 256 * 4, &w, NULL);
                WriteFile(f, g_block, (DWORD)g_block_len, &w, NULL);
                WriteFile(f, g_prev[best], (DWORD)bytes, &w, NULL);
                WriteFile(f, s->bits, (DWORD)bytes, &w, NULL);
                CloseHandle(f);

                char msg[200], *q = msg;
                lstrcpyA(q, "ball captured: "); q += lstrlenA(q);
                q = put_uint(q, (unsigned long)s->width); *q++ = 'x';
                q = put_uint(q, (unsigned long)s->height);
                lstrcpyA(q, " changed="); q += lstrlenA(q);
                q = put_uint(q, (unsigned long)best_changed);
                lstrcpyA(q, " bbox="); q += lstrlenA(q);
                q = put_uint(q, (unsigned long)(bmaxx - bminx + 1)); *q++ = 'x';
                q = put_uint(q, (unsigned long)(bmaxy - bminy + 1));
                lstrcpyA(q, " at "); q += lstrlenA(q);
                q = put_uint(q, (unsigned long)bminx); *q++ = ',';
                q = put_uint(q, (unsigned long)bminy);
                *q = 0;
                slog(msg);
                g_written = 1;
                g_seq++;
            }
        }
    }

    for (int i = 0; i < g_surf_count; i++) snapshot_surface(i, &g_surf[i]);
    memcpy(g_block, block, sizeof(g_block));
    g_block_len = (int)sizeof(g_block);
}

static void install_fill_hook(void)
{
    BYTE *base = (BYTE *)GetModuleHandleA(NULL);
    BYTE *fn = base + 0x3ad50;
    static const BYTE expect[5] = { 0x83, 0xEC, 0x24, 0x53, 0x55 };

    for (int i = 0; i < 5; i++) {
        if (fn[i] != expect[i]) { slog("fill hook: prologue mismatch, skipped"); return; }
    }

    void *tramp = VirtualAlloc(NULL, 64, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE);
    if (!tramp) { slog("fill hook: no trampoline memory"); return; }

    BYTE *t = (BYTE *)tramp;
    memcpy(t, fn, 5);
    DWORD back = (DWORD)(fn + 5) - (DWORD)(t + 10);   /* rel32 from t+10 (after E9 at t+5) */
    t[5] = 0xE9;
    *(DWORD *)(t + 6) = back;
    fill_trampoline = tramp;

    DWORD old;
    if (VirtualProtect(fn, 16, PAGE_EXECUTE_READWRITE, &old)) {
        DWORD rel = (DWORD)fill_entry - (DWORD)(fn + 5);
        fn[0] = 0xE9;
        *(DWORD *)(fn + 1) = rel;
        VirtualProtect(fn, 16, old, &old);
        slog("fill hook: installed at RVA 0x3ad50");
    } else {
        slog("fill hook: VirtualProtect failed");
    }
}

/* ------------------------------------------------------- import patching */

static int patch_iat(HMODULE mod, const char *dll, const char *name, void *replacement)
{
    BYTE *base = (BYTE *)mod;
    IMAGE_DOS_HEADER *dos = (IMAGE_DOS_HEADER *)base;
    if (dos->e_magic != IMAGE_DOS_SIGNATURE) return 0;
    IMAGE_NT_HEADERS *nt = (IMAGE_NT_HEADERS *)(base + dos->e_lfanew);
    if (nt->Signature != IMAGE_NT_SIGNATURE) return 0;

    IMAGE_DATA_DIRECTORY *dir = &nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
    if (!dir->VirtualAddress) return 0;

    IMAGE_IMPORT_DESCRIPTOR *imp = (IMAGE_IMPORT_DESCRIPTOR *)(base + dir->VirtualAddress);
    int patched = 0;
    for (; imp->Name; imp++) {
        if (!imp->OriginalFirstThunk) continue;
        const char *mod_name = (const char *)(base + imp->Name);
        if (lstrcmpiA(mod_name, dll) != 0 && !strstr(mod_name, dll)) continue;

        IMAGE_THUNK_DATA *thunk = (IMAGE_THUNK_DATA *)(base + imp->OriginalFirstThunk);
        IMAGE_THUNK_DATA *iat = (IMAGE_THUNK_DATA *)(base + imp->FirstThunk);
        for (; thunk->u1.AddressOfData; thunk++, iat++) {
            if (thunk->u1.Ordinal & IMAGE_ORDINAL_FLAG) continue;
            IMAGE_IMPORT_BY_NAME *ibn = (IMAGE_IMPORT_BY_NAME *)(base + thunk->u1.AddressOfData);
            if (lstrcmpA((const char *)ibn->Name, name) != 0) continue;
            DWORD old;
            if (VirtualProtect(&iat->u1.Function, sizeof(void *), PAGE_READWRITE, &old)) {
                iat->u1.Function = (ULONG_PTR)replacement;
                VirtualProtect(&iat->u1.Function, sizeof(void *), old, &old);
                patched++;
            }
        }
    }
    return patched;
}

static void install_hooks(void)
{
    HMODULE exe = GetModuleHandleA(NULL);
    HMODULE gdi = GetModuleHandleA("gdi32.dll");

    if (gdi) {
        real_BitBlt = (pBitBlt)GetProcAddress(gdi, "BitBlt");
        real_StretchBlt = (pStretchBlt)GetProcAddress(gdi, "StretchBlt");
        real_CreateDIBSection = (pCreateDIBSection)GetProcAddress(gdi, "CreateDIBSection");
        real_CreatePalette = (pCreatePalette)GetProcAddress(gdi, "CreatePalette");
    }

    int n = 0;
    n += patch_iat(exe, "winmm.dll", "timeGetTime", (void *)hook_timeGetTime);
    n += patch_iat(exe, "kernel32.dll", "GetTickCount", (void *)hook_GetTickCount);
    n += patch_iat(exe, "kernel32.dll", "QueryPerformanceCounter", (void *)hook_QueryPerformanceCounter);
    n += patch_iat(exe, "kernel32.dll", "QueryPerformanceFrequency", (void *)hook_QueryPerformanceFrequency);
    n += patch_iat(exe, "gdi32.dll", "BitBlt", (void *)hook_BitBlt);
    n += patch_iat(exe, "gdi32.dll", "StretchBlt", (void *)hook_StretchBlt);
    n += patch_iat(exe, "gdi32.dll", "CreateDIBSection", (void *)hook_CreateDIBSection);
    n += patch_iat(exe, "gdi32.dll", "CreatePalette", (void *)hook_CreatePalette);

    char buf[96], *p = buf;
    lstrcpyA(p, "hooks installed: "); p += lstrlenA(p);
    p = put_uint(p, (unsigned long)n);
    *p = 0;
    slog(buf);
}

BOOL WINAPI DllMain(HINSTANCE hinst, DWORD reason, LPVOID reserved)
{
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        slog("proxy attach");
        install_hooks();
        /* The inline hook rewrites game code. It is for controlled experiments
         * only; enabled explicitly so a normal session runs untouched. */
        {
            char envbuf[8];
            if (GetEnvironmentVariableA("PETZ_FILL_HOOK", envbuf, sizeof(envbuf)) > 0)
                install_fill_hook();
            else
                slog("fill hook: disabled (set PETZ_FILL_HOOK=1 to enable)");
        }
        DisableThreadLibraryCalls(hinst);
    }
    return TRUE;
}
