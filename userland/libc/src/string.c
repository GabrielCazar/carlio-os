#include <string.h>
#include <stdint.h>

void *memcpy(void *restrict dst, const void *restrict src, size_t n) {
    unsigned char *d = dst; const unsigned char *s = src;
    for (size_t i = 0; i < n; ++i) d[i] = s[i];
    return dst;
}
void *memmove(void *dst, const void *src, size_t n) {
    unsigned char *d = dst; const unsigned char *s = src;
    if ((uintptr_t)d < (uintptr_t)s) { for (size_t i = 0; i < n; ++i) d[i] = s[i]; }
    else if ((uintptr_t)d > (uintptr_t)s) { while (n) { --n; d[n] = s[n]; } }
    return dst;
}
void *memset(void *dst, int value, size_t n) {
    unsigned char *d = dst; for (size_t i = 0; i < n; ++i) d[i] = (unsigned char)value; return dst;
}
int memcmp(const void *a, const void *b, size_t n) {
    const unsigned char *x = a, *y = b;
    for (size_t i = 0; i < n; ++i) if (x[i] != y[i]) return x[i] < y[i] ? -1 : 1;
    return 0;
}
size_t strlen(const char *s) { size_t n = 0; while (s[n]) ++n; return n; }
int strcmp(const char *a, const char *b) {
    while (*a && *a == *b) { ++a; ++b; }
    return (unsigned char)*a < (unsigned char)*b ? -1 : (unsigned char)*a != (unsigned char)*b;
}
int strncmp(const char *a, const char *b, size_t n) {
    for (size_t i = 0; i < n; ++i) { unsigned char x=a[i], y=b[i]; if (x != y || !x) return x < y ? -1 : x != y; }
    return 0;
}
char *strcpy(char *restrict dst, const char *restrict src) {
    char *out = dst; while ((*dst++ = *src++)) {} return out;
}
char *strncpy(char *restrict dst, const char *src, size_t n) {
    size_t i = 0; for (; i < n && src[i]; ++i) dst[i] = src[i]; for (; i < n; ++i) dst[i] = 0; return dst;
}
