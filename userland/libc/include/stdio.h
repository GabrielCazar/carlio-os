#ifndef CARLIO_STDIO_H
#define CARLIO_STDIO_H
#include <stddef.h>
int putchar(int c);
int puts(const char *s);
int printf(const char *format, ...);
int snprintf(char *buffer, size_t size, const char *format, ...);
#endif
