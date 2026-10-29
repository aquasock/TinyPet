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

typedef const char *(__cdecl *pNameFn)(int);
static void *find_export(const char *want);

/* In-memory record ring: fill-time work must never touch the filesystem.
 * The flusher thread writes it out every few seconds. */
#define RING_REC 72
#define RING_RECS (64 * 1024)
static unsigned char *g_ring = NULL;
static volatile LONG g_ring_pos = 0;

static void dump_ring(void)
{
    if (!g_ring) return;
    LONG pos = g_ring_pos;
    if (pos > RING_RECS) pos = RING_RECS;
    HANDLE f = CreateFileA("blocks.bin", GENERIC_WRITE, FILE_SHARE_READ,
                           NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (f == INVALID_HANDLE_VALUE) return;
    DWORD n, total = (DWORD)pos * RING_REC;
    WriteFile(f, g_ring, total, &n, NULL);
    CloseHandle(f);
}


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
    HBITMAP handle;               /* to invalidate on DeleteObject */
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


    char buf[320], *p = buf;
    lstrcpyA(p, "present "); p += lstrlenA(p);
    lstrcpyA(p, api); p += lstrlenA(api);
    *p++ = ' ';
    p = put_uint(p, (unsigned long)frame);
    *p++ = ' ';
    p = put_uint(p, (unsigned long)cx);
    *p++ = 'x';
    p = put_uint(p, (unsigned long)cy);

    /* Do NOT read surface memory here. A surface recorded at
     * CreateDIBSection time may have been deleted since, and hashing its
     * bits() was crashing the game on a real desktop. Log geometry only. */
    for (int i = 0; i < g_surf_count; i++) {
        Surface *s = &g_surf[i];
        if (!s->bits || s->width != cx || s->height != cy) continue;
        lstrcpyA(p, " |"); p += lstrlenA(p);
        p = put_uint(p, (unsigned long)s->bpp);
        lstrcpyA(p, "bpp"); p += lstrlenA(p);
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
        s->handle = (HBITMAP)r;
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

/* ------------------------------------------------------- file access trace */
/* Answers "why is content missing?" with evidence: every failed open, logged
 * with its path and error, instead of guessing at demo mode. */

typedef HANDLE (WINAPI *pCreateFileA)(LPCSTR, DWORD, DWORD, LPSECURITY_ATTRIBUTES,
                                      DWORD, DWORD, HANDLE);
static pCreateFileA real_CreateFileA = NULL;
static volatile LONG g_missing = 0;

static HANDLE WINAPI hook_CreateFileA(LPCSTR name, DWORD access, DWORD share,
                                      LPSECURITY_ATTRIBUTES sa, DWORD disp,
                                      DWORD flags, HANDLE tmpl)
{
    HANDLE h = real_CreateFileA ? real_CreateFileA(name, access, share, sa, disp, flags, tmpl)
                                : INVALID_HANDLE_VALUE;
    if (h == INVALID_HANDLE_VALUE && name && g_missing < 200) {
        DWORD err = GetLastError();
        /* Ignore the noise from probing for optional things in system dirs. */
        char buf[300], *p = buf;
        lstrcpyA(p, "missing: "); p += lstrlenA(p);
        for (const char *q = name; *q && p < buf + 240; q++) *p++ = *q;
        lstrcpyA(p, " err="); p += lstrlenA(p);
        p = put_uint(p, (unsigned long)err);
        *p = 0;
        slog(buf);
        InterlockedIncrement(&g_missing);
    }
    return h;
}

/* -------------------------------------------------------- surface lifetime */

typedef BOOL (WINAPI *pDeleteObject)(HGDIOBJ);
static pDeleteObject real_DeleteObject = NULL;

static BOOL WINAPI hook_DeleteObject(HGDIOBJ obj)
{
    for (int i = 0; i < g_surf_count; i++) {
        if (g_surf[i].handle && (HGDIOBJ)g_surf[i].handle == obj) {
            g_surf[i].bits = NULL;      /* never touch this memory again */
            g_surf[i].handle = NULL;
        }
    }
    return real_DeleteObject ? real_DeleteObject(obj) : TRUE;
}

/* ------------------------------------------------------------- profiler */
static void flush_dispatch(void);
/* perf is unusable here (perf_event_paranoid=4, no sudo), so we sample the main
 * thread's EIP from inside the process. Coverage, not exact timing: 1 kHz over
 * a session is plenty to rank which of the 5,719 methods actually run. */

#define PROF_BASE 0x400000u
#define PROF_SIZE 0x200000u
#define PROF_SLOTS (PROF_SIZE / 4)

static unsigned int *g_hist = NULL;
static DWORD g_main_tid = 0;
static volatile LONG g_prof_on = 0;
static int g_do_sample = 0;

typedef DWORD (WINAPI *pTimeBeginPeriod)(UINT);
typedef DWORD (WINAPI *pTimeEndPeriod)(UINT);

static DWORD WINAPI profiler_thread(LPVOID unused)
{
    (void)unused;
    /* Wine's Sleep(1) is coarse; ask for 1 ms timer resolution so the sampler
     * actually runs near 1 kHz instead of ~10 Hz. */
    HMODULE winmm = LoadLibraryA("winmm.dll");
    pTimeBeginPeriod begin = winmm ? (pTimeBeginPeriod)GetProcAddress(winmm, "timeBeginPeriod") : NULL;
    if (begin) begin(1);
    HANDLE h = OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION,
                          FALSE, g_main_tid);
    if (!h) { slog("profiler: cannot open main thread"); return 1; }

    DWORD last_flush = GetTickCount();
    /* One-shot: enumerate the behaviour registry so ids can be named offline. */
    {
        char dn[8];
        if (GetEnvironmentVariableA("PETZ_DUMPNAMES", dn, sizeof(dn)) > 0) {
            Sleep(15000);   /* the registry is built during game startup, not at load */
            pNameFn gf = (pNameFn)find_export("?GetGoalStringName@SBehaviorRegistry@@SAPBDH@Z");
            pNameFn pf = (pNameFn)find_export("?GetPlanStringName@SBehaviorRegistry@@SAPBDH@Z");
            HANDLE f = CreateFileA("names.txt", GENERIC_WRITE, FILE_SHARE_READ, NULL,
                                   CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
            if (f != INVALID_HANDLE_VALUE) {
                DWORD n; char line[96];
                for (int k = 0; k < 2; k++) {
                    pNameFn fn = k ? pf : gf;
                    if (!fn) continue;
                    for (int i = 0; i < 200; i++) {
                        const char *nm = fn(i);
                        if (!nm) continue;
                        char *p2 = line;
                        p2 = put_uint(p2, (unsigned long)k); *p2++ = ' ';
                        p2 = put_uint(p2, (unsigned long)i); *p2++ = ' ';
                        for (const char *q = nm; *q && p2 < line + 80; q++) *p2++ = *q;
                        *p2++ = '\n';
                        WriteFile(f, line, (DWORD)(p2 - line), &n, NULL);
                    }
                }
                CloseHandle(f);
                slog("names dumped");
            }
            return 0;
        }
    }

    for (;;) {
        Sleep(5);
        if (!g_do_sample) { flush_dispatch(); dump_ring(); continue; }   /* ~200 Hz: 1 kHz suspend/resume perturbed the target */
        if (SuspendThread(h) == (DWORD)-1) continue;
        CONTEXT c;
        c.ContextFlags = CONTEXT_CONTROL;
        if (GetThreadContext(h, &c)) {
            DWORD ip = (DWORD)c.Eip;
            if (ip >= PROF_BASE && ip < PROF_BASE + PROF_SIZE)
                g_hist[(ip - PROF_BASE) >> 2]++;
        }
        ResumeThread(h);

        if (GetTickCount() - last_flush > 5000) {
            flush_dispatch();
            last_flush = GetTickCount();
            HANDLE f = CreateFileA("profile.bin", GENERIC_WRITE, FILE_SHARE_READ,
                                   NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
            if (f != INVALID_HANDLE_VALUE) {
                DWORD n;
                WriteFile(f, g_hist, PROF_SLOTS * 4, &n, NULL);
                CloseHandle(f);
            }
        }
    }
    return 0;
}

static void start_profiler(void)
{
    char envbuf[8];
    {
        char bl[8];
        if (GetEnvironmentVariableA("PETZ_BLOCKLOG", bl, sizeof(bl)) > 0) {
            g_ring = (unsigned char *)VirtualAlloc(NULL, (size_t)RING_RECS * RING_REC,
                        MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
            if (g_ring) slog("blocklog: ring allocated");
        }
    }
    if (GetEnvironmentVariableA("PETZ_PROFILE", envbuf, sizeof(envbuf)) > 0) g_do_sample = 1;
    char tr[8];
    if (GetEnvironmentVariableA("PETZ_TRACE", tr, sizeof(tr)) > 0) g_do_sample = g_do_sample ? 1 : 0;
    if (!g_do_sample && GetEnvironmentVariableA("PETZ_TRACE", tr, sizeof(tr)) == 0) return;

    g_hist = (unsigned int *)VirtualAlloc(NULL, PROF_SLOTS * 4,
                                          MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (!g_hist) { slog("profiler: no memory"); return; }

    g_main_tid = GetCurrentThreadId();
    HANDLE t = CreateThread(NULL, 0, profiler_thread, NULL, 0, NULL);
    if (t) { CloseHandle(t); g_prof_on = 1; slog("profiler: sampling main thread"); }
    else slog("profiler: thread create failed");
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

extern void tex_entry(void);
extern void serial_entry(void);
extern void goal_entry(void);
extern void frame_entry(void);
extern void disp_entry(void);
extern void fill2_entry(void);
extern void fill3_entry(void);
extern void fill4_entry(void);
extern void fill5_entry(void);
void *g_fill2_tramp = NULL, *g_fill3_tramp = NULL;
void *g_fill4_tramp = NULL, *g_fill5_tramp = NULL;
void *g_disp_tramp = NULL;
static int g_disp_dumps = 0;

void disp_probe(void *ballstates)
{
    /* Frame marker in the block ring: DisplayBallzFrame runs once per pet per
     * frame, so this is the clean boundary the fill stream otherwise lacks. */
    if (g_ring) {
        LONG slot = InterlockedIncrement(&g_ring_pos) - 1;
        if (slot < RING_RECS) {
            unsigned char *rec = g_ring + (size_t)slot * RING_REC;
            memset(rec, 0, RING_REC);
            short sentinel = -1;
            memcpy(rec + 4, &sentinel, 2);       /* colour field = -1 marks a frame */
            DWORD marker = 0x4652414d;           /* 'M','A','R','F' */
            memcpy(rec + 64, &marker, 4);
        }
    }
    if (!ballstates || g_disp_dumps >= 3) return;
    char name[48], *q = name;
    lstrcpyA(q, "ballstate-"); q += lstrlenA(q);
    q = put_uint(q, (unsigned long)g_disp_dumps);
    lstrcpyA(q, ".raw"); q += lstrlenA(q);
    *q = 0;
    HANDLE f = CreateFileA(name, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS,
                           FILE_ATTRIBUTE_NORMAL, NULL);
    if (f == INVALID_HANDLE_VALUE) return;
    DWORD n;
    WriteFile(f, ballstates, 2048, &n, NULL);
    CloseHandle(f);
    g_disp_dumps++;
    char buf[80], *p = buf;
    lstrcpyA(p, "ballstates dumped -> "); p += lstrlenA(p);
    lstrcpynA(p, name, 30);
    slog(buf);
}
extern void frame_post_entry(void);
void *g_frame_tramp = NULL, *g_frame_ret = NULL;
void *g_frame_post = (void *)frame_post_entry;
static volatile LONG g_frame_calls = 0;
static int g_frame_dumps = 0;

void frame_probe(void *self, long idx)
{
    static volatile LONG n = 0;
    if (InterlockedIncrement(&n) > 4) return;
    char buf[96], *p = buf;
    lstrcpyA(p, "ballframe idx="); p += lstrlenA(p);
    p = put_uint(p, (unsigned long)(idx < 0 ? 0 : idx));
    lstrcpyA(p, " self="); p += lstrlenA(p);
    p = put_hex64(p, (unsigned long long)(ULONG_PTR)self);
    *p = 0; slog(buf);
}

void frame_post_probe(unsigned ret)
{
    if (!ret || g_frame_dumps >= 4) return;
    char name[48], *q = name;
    lstrcpyA(q, "ballframe-"); q += lstrlenA(q);
    q = put_uint(q, (unsigned long)g_frame_dumps);
    lstrcpyA(q, ".raw"); q += lstrlenA(q);
    *q = 0;
    HANDLE f = CreateFileA(name, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS,
                           FILE_ATTRIBUTE_NORMAL, NULL);
    if (f == INVALID_HANDLE_VALUE) return;
    DWORD n;
    /* 512 bytes is enough to see the first fields; enough to match against BDT. */
    WriteFile(f, (const void *)(ULONG_PTR)ret, 512, &n, NULL);
    CloseHandle(f);
    g_frame_dumps++;
    char buf[80], *p = buf;
    lstrcpyA(p, "ballframe dumped -> "); p += lstrlenA(p);
    lstrcpynA(p, name, 30);
    slog(buf);
}
extern void plan_entry(void);
extern void state_entry(void);
void *g_goal_tramp = NULL, *g_plan_tramp = NULL, *g_state_tramp = NULL;
static volatile LONG g_dispatch[3] = {0,0,0};
#define DISP_SLOTS 512
static unsigned int g_disp_goal[DISP_SLOTS];   /* counts by goal id  */
static unsigned int g_disp_plan[DISP_SLOTS];   /* counts by plan id  */
static volatile LONG g_disp_total[3] = {0,0,0};
static pNameFn g_goal_name_fn = NULL, g_plan_name_fn = NULL;
static const char *g_goal_name[DISP_SLOTS];
static const char *g_plan_name[DISP_SLOTS];

static void flush_dispatch(void)
{
    HANDLE f = CreateFileA("dispatch.txt", GENERIC_WRITE, FILE_SHARE_READ,
                           NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (f == INVALID_HANDLE_VALUE) return;
    DWORD n;
    char line[64];
    for (int k = 0; k < 2; k++) {
        unsigned int *tab = k ? g_disp_plan : g_disp_goal;
        for (int i = 0; i < DISP_SLOTS; i++) {
            if (!tab[i]) continue;
            char *p = line;
            p = put_uint(p, (unsigned long)k);
            *p++ = ' ';
            p = put_uint(p, (unsigned long)i);
            *p++ = ' ';
            p = put_uint(p, (unsigned long)tab[i]);
            *p++ = ' ';
            const char *nm = k ? g_plan_name[i] : g_goal_name[i];
            if (nm) for (const char *q = nm; *q && p < line + 48; q++) *p++ = *q;
            *p++ = '\n';
            WriteFile(f, line, (DWORD)(p - line), &n, NULL);
        }
    }
    CloseHandle(f);
}

void dispatch_probe(int kind, void *token, int arg)
{
    LONG n = InterlockedIncrement(&g_dispatch[kind & 3]);
    InterlockedIncrement(&g_disp_total[kind & 3]);
    if ((kind == 0 || kind == 1) && arg >= 0 && arg < DISP_SLOTS) {
        unsigned int *tabp = (kind == 1) ? g_disp_plan : g_disp_goal;
        InterlockedIncrement((volatile LONG *)&tabp[arg]);
        /* cache the human name; the registry stores it at object+0x1c */
        const char **slot = (kind == 1) ? &g_plan_name[arg] : &g_goal_name[arg];
        if (!*slot) {
            pNameFn fn = (kind == 1) ? g_plan_name_fn : g_goal_name_fn;
            if (fn) *slot = fn(arg);
        }
    }
    if (n > 6) return;
    /* Only GoalToken and PlanToken are pointers. DispatchState's second and
     * third arguments are bools - dereferencing them faults (c0000005). */
    int a = -1, b = -1;
    if (kind != 2 && token) {
        MEMORY_BASIC_INFORMATION mbi;
        if (VirtualQuery(token, &mbi, sizeof(mbi)) &&
            mbi.State == MEM_COMMIT && !(mbi.Protect & PAGE_NOACCESS)) {
            a = *(int *)token;
            b = *((int *)token + 1);
        }
    } else if (kind == 2) {
        a = (int)(INT_PTR)token;
    }
    char buf[160], *p = buf;
    lstrcpyA(p, "dispatch "); p += lstrlenA(p);
    *p++ = (char)('0' + (kind % 3));
    lstrcpyA(p, " n="); p += lstrlenA(p);
    p = put_uint(p, (unsigned long)n);
    lstrcpyA(p, " tok="); p += lstrlenA(p);
    p = put_hex64(p, (unsigned long long)(ULONG_PTR)token);
    lstrcpyA(p, " f0="); p += lstrlenA(p);
    p = put_uint(p, (unsigned long)(a < 0 ? 0 : a));
    lstrcpyA(p, " f1="); p += lstrlenA(p);
    p = put_uint(p, (unsigned long)(b < 0 ? 0 : b));
    lstrcpyA(p, " arg="); p += lstrlenA(p);
    p = put_uint(p, (unsigned long)(arg < 0 ? 0 : arg));
    *p = 0;
    slog(buf);
}
void *g_serial_tramp = NULL;
static volatile LONG g_serial_calls = 0;
extern void tex_post_entry(void);
void *g_tex_ret = NULL;
void *g_tex_out = NULL;      /* int* passed by the caller (row offset) */
void *g_tex_post = (void *)tex_post_entry;
void *g_tex_tramp = NULL;
static volatile LONG g_tex_calls = 0;

void tex_probe(void *drawport, void *block)
{
    LONG n = InterlockedIncrement(&g_tex_calls);
    if (n > 3) return;
    char buf[128], *p = buf;
    lstrcpyA(p, "tex#"); p += lstrlenA(p);
    p = put_uint(p, (unsigned long)n);
    lstrcpyA(p, " drawport="); p += lstrlenA(p);
    p = put_hex64(p, (unsigned long long)(ULONG_PTR)drawport);
    lstrcpyA(p, " block="); p += lstrlenA(p);
    p = put_hex64(p, (unsigned long long)(ULONG_PTR)block);
    *p = 0;
    slog(buf);
}

void serial_probe(const char *serial, int mode)
{
    LONG n = InterlockedIncrement(&g_serial_calls);
    if (n > 6) return;
    char buf[128], *p = buf;
    lstrcpyA(p, "serial#"); p += lstrlenA(p);
    p = put_uint(p, (unsigned long)n);
    lstrcpyA(p, " mode="); p += lstrlenA(p);
    p = put_uint(p, (unsigned long)mode);
    lstrcpyA(p, " len="); p += lstrlenA(p);
    p = put_uint(p, (unsigned long)(serial ? lstrlenA(serial) : 0));
    lstrcpyA(p, " val=["); p += lstrlenA(p);
    if (serial) {
        for (int i = 0; i < 16 && serial[i]; i++) *p++ = serial[i];
    }
    *p++ = ']';
    *p = 0;
    slog(buf);
}

void tex_post_probe(unsigned ret)
{
    static volatile LONG posts = 0;
    if (InterlockedIncrement(&posts) > 3) return;

    int out_val = 0;
    if (g_tex_out) out_val = *(int *)g_tex_out;

    char buf[192], *p = buf;
    lstrcpyA(p, "texret#"); p += lstrlenA(p);
    p = put_uint(p, (unsigned long)posts);
    lstrcpyA(p, " tex="); p += lstrlenA(p);
    p = put_hex64(p, (unsigned long long)ret);
    lstrcpyA(p, " out="); p += lstrlenA(p);
    p = put_uint(p, (unsigned long)(out_val < 0 ? 0 : out_val));
    lstrcpyA(p, " bytes="); p += lstrlenA(p);
    if (ret) {
        const unsigned char *b = (const unsigned char *)(ULONG_PTR)ret;
        for (int i = 0; i < 8; i++) {
            *p++ = "0123456789abcdef"[b[i] >> 4];
            *p++ = "0123456789abcdef"[b[i] & 0xF];
        }
    }
    *p = 0;
    slog(buf);
}

/* Install a rel32 jump at rva, replaying `len` whole prologue bytes from a
 * trampoline. Returns the trampoline pointer, or NULL if the bytes do not
 * match (so we never patch a build we do not recognise). */
/* Resolve an exported method by its mangled name. Addresses move between builds
 * (the 4.2 patch shifts XFillRotatedCircle from 0x3ad50 to 0x5cc10), but the
 * export table is stable, so name lookup makes the hooks build-independent. */
static void *find_export(const char *want)
{
    BYTE *base = (BYTE *)GetModuleHandleA(NULL);
    IMAGE_DOS_HEADER *dos = (IMAGE_DOS_HEADER *)base;
    if (dos->e_magic != IMAGE_DOS_SIGNATURE) return NULL;
    IMAGE_NT_HEADERS *nt = (IMAGE_NT_HEADERS *)(base + dos->e_lfanew);
    IMAGE_DATA_DIRECTORY *dir = &nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXPORT];
    if (!dir->VirtualAddress) return NULL;
    IMAGE_EXPORT_DIRECTORY *exp = (IMAGE_EXPORT_DIRECTORY *)(base + dir->VirtualAddress);
    DWORD *names = (DWORD *)(base + exp->AddressOfNames);
    WORD *ords = (WORD *)(base + exp->AddressOfNameOrdinals);
    DWORD *funcs = (DWORD *)(base + exp->AddressOfFunctions);
    for (DWORD i = 0; i < exp->NumberOfNames; i++) {
        if (!lstrcmpA((const char *)(base + names[i]), want))
            return base + funcs[ords[i]];
    }
    return NULL;
}

static void *install_inline_ex(DWORD rva, void *entry, const BYTE *expect, int expect_len, int steal)
{
    BYTE *base = (BYTE *)GetModuleHandleA(NULL);
    BYTE *fn = base + rva;
    for (int i = 0; i < expect_len; i++)
        if (fn[i] != expect[i]) { slog("inline hook: prologue mismatch"); return NULL; }
    const int len = steal;

    BYTE *tramp = (BYTE *)VirtualAlloc(NULL, 64, MEM_COMMIT | MEM_RESERVE,
                                       PAGE_EXECUTE_READWRITE);
    if (!tramp) return NULL;
    memcpy(tramp, fn, len);
    tramp[len] = 0xE9;
    *(DWORD *)(tramp + len + 1) = (DWORD)(fn + len) - (DWORD)(tramp + len + 5);

    DWORD old;
    if (!VirtualProtect(fn, 32, PAGE_EXECUTE_READWRITE, &old)) return NULL;
    fn[0] = 0xE9;
    *(DWORD *)(fn + 1) = (DWORD)entry - (DWORD)(fn + 5);
    VirtualProtect(fn, 32, old, &old);
    return tramp;
}

static void *install_inline(DWORD rva, void *entry, const BYTE *expect, int len)
{
    return install_inline_ex(rva, entry, expect, len, len);
}

/* The body is drawn mostly through the non-rotated circle fills, not
 * XFillRotatedCircle. All five take a CircleRenderBlock*, so they share the
 * same probe. */
static void install_other_circle_hooks(void)
{
    struct { const char *name; void *entry; void **tramp; int steal; } t[] = {
        { "?XFillNormalCircle@XDrawPort@@QAEXPBVCircleRenderBlock@@@Z",
          (void*)fill2_entry, &g_fill2_tramp, 8 },
        { "?XFillEmbossCircle@XDrawPort@@QAEXPBVCircleRenderBlock@@@Z",
          (void*)fill3_entry, &g_fill3_tramp, 5 },
        { "?XFillOutlineCircle@XDrawPort@@QAEXPBVCircleRenderBlock@@@Z",
          (void*)fill4_entry, &g_fill4_tramp, 5 },
        { "?XFillCircleEx@XDrawPort@@QAE_NPAVCircleRenderBlock@@@Z",
          (void*)fill5_entry, &g_fill5_tramp, 5 },
    };
    static const BYTE pre[2] = { 0x83, 0xEC };
    for (int i = 0; i < 4; i++) {
        void *fn = find_export(t[i].name);
        if (!fn) { slog("circle hook: export not found"); continue; }
        DWORD rva = (DWORD)fn - (DWORD)GetModuleHandleA(NULL);
        *t[i].tramp = install_inline_ex(rva, t[i].entry, pre, 2, t[i].steal);
        slog(*t[i].tramp ? "circle hook: installed" : "circle hook: prologue mismatch");
    }
}

static void install_tex_hook(void)
{
    static const BYTE expect[8] = { 0x83,0xEC,0x24, 0x53, 0x8B,0x5C,0x24,0x2C };
    /* Resolve by name so the same shim works on 4.00.01 and 4.2. */
    {
        void *fn = find_export("?_initTextureDrawing@XDrawPort@@QBEPBEABVCircleRenderBlock@@HAAJ@Z");
        if (fn) {
            DWORD rva = (DWORD)fn - (DWORD)GetModuleHandleA(NULL);
            g_tex_tramp = install_inline(rva, (void *)tex_entry, expect, 8);
            slog(g_tex_tramp ? "tex hook: installed (by name)" : "tex hook: prologue mismatch");
        } else slog("tex hook: export not found");
    }
    {
        void *fn = find_export("?ValidRetailSerialNumber@XSecure@@SA_NPBDW4ERunMode@@@Z");
        if (fn) {
            static const BYTE sp[8] = { 0x83,0xEC,0x28, 0xA0,0x88,0x0B,0x64,0x00 };
            DWORD rva = (DWORD)fn - (DWORD)GetModuleHandleA(NULL);
            g_serial_tramp = install_inline(rva, (void *)serial_entry, sp, 8);
            slog(g_serial_tramp ? "serial hook: installed (by name)" : "serial hook: prologue mismatch");
        } else slog("serial hook: export not found");
    }
    slog(g_tex_tramp ? "tex hook: installed at RVA 0x39f10"
                     : "tex hook: not installed");
}

extern void fill_entry(void);
void *fill_trampoline = NULL;

static volatile LONG g_fill_calls = 0;
static unsigned char *g_prev[MAX_SURFACES];
static int g_have_prev = 0;
static int g_written = 0;
static int g_seq = 0;
#define MAX_SEQ 24   /* ring buffer: always the most recent N draws */
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
    /* Recording every fill's render block is opt-in: doing file I/O on the
     * render thread with this frequency crashed the retail game (c0000005 via
     * Verbassert). The safe path is the interval capture further down. */
    if (g_ring && block) {
        LONG slot = InterlockedIncrement(&g_ring_pos) - 1;
        if (slot < RING_RECS) {
            unsigned char *rec = g_ring + (size_t)slot * RING_REC;
            memcpy(rec, block, 64);
            DWORD dp = (DWORD)(ULONG_PTR)drawport;
            memcpy(rec + 64, &dp, 4);
            DWORD frame = (DWORD)n;
            memcpy(rec + 68, &frame, 4);
        }
    }
    if (!block) return;

    /* No per-call logging: this runs thousands of times a second and a single
     * slog() here (CreateFile + WriteFile) crashed the retail game. */

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
    {   /* ring buffer: always record, slot index wraps */
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
        if (best >= 0) {
            char lb[160], *lq = lb;
            lstrcpyA(lq, "fill interval: "); lq += lstrlenA(lq);
            lq = put_uint(lq, (unsigned long)best_changed);
            lstrcpyA(lq, " px, bbox "); lq += lstrlenA(lq);
            lq = put_uint(lq, (unsigned long)(bmaxx - bminx + 1)); *lq++ = 'x';
            lq = put_uint(lq, (unsigned long)(bmaxy - bminy + 1));
            lstrcpyA(lq, " at "); lq += lstrlenA(lq);
            lq = put_uint(lq, (unsigned long)bminx); *lq++ = ',';
            lq = put_uint(lq, (unsigned long)bminy);
            *lq = 0;
            slog(lb);
        }
        /* Only keep substantial draws: startup floods the budget with tiny
         * cursor/title fills. 1500 px is comfortably above a small disc. */
        if (best >= 0 && best_changed >= 1500) {
            Surface *s = &g_surf[best];
            size_t bytes = (size_t)s->stride * s->height;
            char fname[64], *fp = fname;
            lstrcpyA(fp, "ball-input-"); fp += lstrlenA(fp);
            fp = put_uint(fp, (unsigned long)(g_seq % MAX_SEQ));
            lstrcpyA(fp, ".raw"); fp += lstrlenA(fp);
            *fp = 0;
            HANDLE f = CreateFileA(fname, GENERIC_WRITE, 0, NULL,
                                   CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
            if (f != INVALID_HANDLE_VALUE) {
                DWORD w, hdr[10] = {
                    (DWORD)s->width, (DWORD)s->height, (DWORD)s->bpp,
                    (DWORD)s->stride, (DWORD)bminx, (DWORD)bminy,
                    (DWORD)(bmaxx - bminx + 1), (DWORD)(bmaxy - bminy + 1),
                    (DWORD)best_changed, (DWORD)g_fill_calls
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
    static const BYTE expect[5] = { 0x83, 0xEC, 0x24, 0x53, 0x55 };
    BYTE *fn = (BYTE *)find_export("?XFillRotatedCircle@XDrawPort@@QAE_NPBVCircleRenderBlock@@@Z");
    if (!fn) { slog("fill hook: export not found"); return; }
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
        real_DeleteObject = (pDeleteObject)GetProcAddress(gdi, "DeleteObject");
    }
    {
        HMODULE k32 = GetModuleHandleA("kernel32.dll");
        if (k32) real_CreateFileA = (pCreateFileA)GetProcAddress(k32, "CreateFileA");
    }

    int n = 0;
    /* Clock pinning is for deterministic capture runs only. Profiling wants the
     * game running at its natural speed, so it is opt-in. */
    {
        char pin[8];
        if (GetEnvironmentVariableA("PETZ_PIN_CLOCK", pin, sizeof(pin)) > 0) {
            n += patch_iat(exe, "winmm.dll", "timeGetTime", (void *)hook_timeGetTime);
            n += patch_iat(exe, "kernel32.dll", "GetTickCount", (void *)hook_GetTickCount);
            n += patch_iat(exe, "kernel32.dll", "QueryPerformanceCounter", (void *)hook_QueryPerformanceCounter);
            n += patch_iat(exe, "kernel32.dll", "QueryPerformanceFrequency", (void *)hook_QueryPerformanceFrequency);
        } else {
            slog("clock: not pinned (set PETZ_PIN_CLOCK=1 for capture runs)");
        }
    }
    n += patch_iat(exe, "gdi32.dll", "BitBlt", (void *)hook_BitBlt);
    n += patch_iat(exe, "gdi32.dll", "StretchBlt", (void *)hook_StretchBlt);
    n += patch_iat(exe, "gdi32.dll", "CreateDIBSection", (void *)hook_CreateDIBSection);
    n += patch_iat(exe, "gdi32.dll", "CreatePalette", (void *)hook_CreatePalette);
    n += patch_iat(exe, "gdi32.dll", "DeleteObject", (void *)hook_DeleteObject);
    n += patch_iat(exe, "kernel32.dll", "CreateFileA", (void *)hook_CreateFileA);

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
        start_profiler();
        install_hooks();
        /* The inline hook rewrites game code. It is for controlled experiments
         * only; enabled explicitly so a normal session runs untouched. */
        {
            char envbuf[8];
            char tr[8];
            if (GetEnvironmentVariableA("PETZ_TRACE", tr, sizeof(tr)) > 0) {
                struct { const char *name; void *entry; void **tramp; BYTE pre[4]; } t[] = {
                    { "?DispatchGoal@SBehaviorRegistry@@SAXAAVCharacterSprite@@AAVGoalToken@@H@Z",
                      (void *)goal_entry, &g_goal_tramp, {0x8b,0x44,0x24,0x0c} },
                    { "?DispatchPlan@SBehaviorRegistry@@SAXAAVCharacterSprite@@AAVPlanToken@@H@Z",
                      (void *)plan_entry, &g_plan_tramp, {0x8b,0x44,0x24,0x0c} },
                    { "?DispatchState@SBehaviorRegistry@@SAXAAVCharacterSprite@@_N1H@Z",
                      (void *)state_entry, &g_state_tramp, {0x8b,0x44,0x24,0x10} },
                };
                {
                    char fh[8];
                    void *fn = GetEnvironmentVariableA("PETZ_FRAME_HOOK", fh, sizeof(fh)) > 0
                        ? find_export("?GetRawFramePtr@XBallzData@@QAEPAUBallFrame@@J@Z") : NULL;
                    if (fn) {
                        static const BYTE pre[5] = {0x55,0x8B,0xEC,0x6A,0xFF};
                        DWORD rva = (DWORD)fn - (DWORD)GetModuleHandleA(NULL);
                        g_frame_tramp = install_inline_ex(rva, (void *)frame_entry, pre, 5, 5);
                        slog(g_frame_tramp ? "frame hook: installed" : "frame hook: prologue mismatch");
                    } else slog("frame hook: export not found");
                }
                {
                    void *fn = find_export("?DisplayBallzFrame@XBallz@@QAEXPAVXDrawPort@@PBU?$XTRect@HJ@@PAVBallState@@@Z");
                    if (fn) {
                        static const BYTE pre[2] = {0x6A,0xFF};
                        DWORD rva = (DWORD)fn - (DWORD)GetModuleHandleA(NULL);
                        g_disp_tramp = install_inline_ex(rva, (void *)disp_entry, pre, 2, 7);
                        slog(g_disp_tramp ? "ballstate hook: installed" : "ballstate hook: prologue mismatch");
                    } else slog("ballstate hook: export not found");
                }
                g_goal_name_fn = (pNameFn)find_export("?GetGoalStringName@SBehaviorRegistry@@SAPBDH@Z");
                g_plan_name_fn = (pNameFn)find_export("?GetPlanStringName@SBehaviorRegistry@@SAPBDH@Z");
                for (int i = 0; i < 3; i++) {
                    void *fn = find_export(t[i].name);
                    if (!fn) { slog("dispatch hook: export not found"); continue; }
                    DWORD rva = (DWORD)fn - (DWORD)GetModuleHandleA(NULL);
                    void *tr2 = install_inline_ex(rva, t[i].entry, t[i].pre, 4, 10);
                    *t[i].tramp = tr2;
                    slog(tr2 ? "dispatch hook: installed" : "dispatch hook: prologue mismatch");
                }
            }
            if (GetEnvironmentVariableA("PETZ_FILL_HOOK", envbuf, sizeof(envbuf)) > 0) {
                install_fill_hook();
                install_other_circle_hooks();
                {
                    char tx[8];
                    if (GetEnvironmentVariableA("PETZ_TEX_HOOK", tx, sizeof(tx)) > 0)
                        install_tex_hook();
                }
            }
            else
                slog("fill hook: disabled (set PETZ_FILL_HOOK=1 to enable)");
        }
        DisableThreadLibraryCalls(hinst);
    }
    return TRUE;
}
