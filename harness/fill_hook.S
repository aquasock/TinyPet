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
