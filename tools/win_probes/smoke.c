#include <windows.h>
#include <stdio.h>
int main(void) {
    printf("NOTIFYICONDATAW size=%zu\n", sizeof(NOTIFYICONDATAW));
    printf("  cbSize offset=%zu\n", offsetof(NOTIFYICONDATAW, cbSize));
    printf("  hWnd offset=%zu\n", offsetof(NOTIFYICONDATAW, hWnd));
    printf("  uID offset=%zu\n", offsetof(NOTIFYICONDATAW, uID));
    printf("  uFlags offset=%zu\n", offsetof(NOTIFYICONDATAW, uFlags));
    printf("  szTip offset=%zu\n", offsetof(NOTIFYICONDATAW, szTip));
    printf("LOGFONTW size=%zu\n", sizeof(LOGFONTW));
    printf("WNDCLASSEXW size=%zu\n", sizeof(WNDCLASSEXW));
    printf("sizeof(WPARAM)=%zu sizeof(LPARAM)=%zu sizeof(LRESULT)=%zu\n", sizeof(WPARAM), sizeof(LPARAM), sizeof(LRESULT));
    return 0;
}
