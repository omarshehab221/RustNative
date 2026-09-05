#include <windows.h>
#include <stdio.h>
int main(void) {
    SetLastError(0);
    LONG_PTR v = GetWindowLongPtrW(NULL, GWLP_USERDATA);
    DWORD err = GetLastError();
    printf("GetWindowLongPtrW(NULL, GWLP_USERDATA) = %ld, GetLastError=%lu\n", (long)v, (unsigned long)err);
    printf("did not crash\n");
    return 0;
}
