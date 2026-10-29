/*
 * Inline hook entry for XDrawPort::XFillRotatedCircle (Petz 4.exe RVA 0x3ad50).
 *
 * The function's first five bytes are:
 *      83 EC 24    sub  esp, 0x24
 *      53          push ebx
 *      55          push ebp
 * a clean instruction boundary, so we steal exactly those 5 bytes for a rel32
 * jump and replay them from a trampoline.
 *
 * At entry: ecx = this (XDrawPort*), [esp+4] = CircleRenderBlock*.
 *
 * No return-address games here. Capturing what a call *produced* is done by
 * comparing the surface snapshot taken at this call against the current surface
 * at the next one - fills are sparse enough that the difference is essentially
 * a single ball, and it avoids depending on the callee's epilogue shape.
 */
    .intel_syntax noprefix
    .text

    .globl _fill_entry
_fill_entry:
    pushad
    pushfd
    /* after pushad(32) + pushfd(4): ret at +36, arg1 at +40, saved ecx at +28 */
    mov     eax, dword ptr [esp+40]
    mov     edx, dword ptr [esp+28]
    push    eax
    push    edx
    call    _fill_probe
    add     esp, 8
    popfd
    popad
    jmp     dword ptr [_fill_trampoline]

/* Second target: _initTextureDrawing (RVA 0x39f10). Prologue is 8 whole bytes:
 *      83 EC 24        sub  esp,0x24
 *      53              push ebx
 *      8B 5C 24 2C     mov  ebx,[esp+0x2c]
 * Used to observe when a fill is textured and which render block it belongs to.
 */
    .globl _tex_entry
_tex_entry:
    pushad
    pushfd
    mov     eax, dword ptr [esp+40]     /* arg1: CircleRenderBlock& */
    mov     edx, dword ptr [esp+28]     /* ecx: this (XDrawPort*) */
    mov     ebx, dword ptr [esp+48]     /* arg3: int& out (row offset) */
    mov     dword ptr [_g_tex_out], ebx
    push    eax
    push    edx
    call    _tex_probe
    add     esp, 8

    /* swap return address for the post stub */
    mov     eax, dword ptr [esp+36]
    mov     dword ptr [_g_tex_ret], eax
    mov     eax, dword ptr [_g_tex_post]
    mov     dword ptr [esp+36], eax

    popfd
    popad
    jmp     dword ptr [_g_tex_tramp]

/* Post-call observation for _initTextureDrawing: swap the return address so we
 * land back in our stub with eax still holding the returned texture pointer. */
    .globl _tex_post_entry
_tex_post_entry:
    pushad
    pushfd
    /* after pushad(32)+pushfd(4), saved EAX sits at [esp+32] */
    mov     eax, dword ptr [esp+32]
    push    eax
    call    _tex_post_probe
    add     esp, 4
    popfd
    popad
    jmp     dword ptr [_g_tex_ret]

/* Entry observation for XSecure::ValidRetailSerialNumber (RVA 0x615c0).
 * Prologue: 83 EC 28 (sub esp,0x28) / A0 88 0B 64 00 (mov al,ds:0x640b88) = 8 bytes */
    .globl _serial_entry
_serial_entry:
    pushad
    pushfd
    mov     eax, dword ptr [esp+40]      /* arg1: const char* serial */
    mov     edx, dword ptr [esp+44]      /* arg2: ERunMode */
    push    eax
    push    edx
    call    _serial_probe
    add     esp, 8
    popfd
    popad
    jmp     dword ptr [_g_serial_tramp]

/* Behaviour dispatch trace. SBehaviorRegistry::DispatchGoal/Plan/State are
 * exported statics, so every behaviour that actually runs passes through them.
 * Prologues are 4-byte + 6-byte movs (10 bytes, whole instructions).
 *   DispatchGoal (const CharacterSprite&, GoalToken&, int)
 *   DispatchPlan (const CharacterSprite&, PlanToken&, int)
 *   DispatchState(const CharacterSprite&, bool, bool, int)
 */
    .globl _goal_entry
_goal_entry:
    pushad
    pushfd
    mov     eax, dword ptr [esp+44]      /* token reference */
    mov     edx, dword ptr [esp+48]      /* int arg */
    push    edx          /* arg  */
    push    eax          /* token */
    push    0            /* kind */
    call    _dispatch_probe
    add     esp, 12
    popfd
    popad
    jmp     dword ptr [_g_goal_tramp]

    .globl _plan_entry
_plan_entry:
    pushad
    pushfd
    mov     eax, dword ptr [esp+44]
    mov     edx, dword ptr [esp+48]
    push    edx
    push    eax
    push    1
    call    _dispatch_probe
    add     esp, 12
    popfd
    popad
    jmp     dword ptr [_g_plan_tramp]

    .globl _state_entry
_state_entry:
    pushad
    pushfd
    mov     eax, dword ptr [esp+44]
    mov     edx, dword ptr [esp+48]
    push    edx
    push    eax
    push    2
    call    _dispatch_probe
    add     esp, 12
    popfd
    popad
    jmp     dword ptr [_g_state_tramp]
