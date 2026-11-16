#include <stdarg.h>
#include <stdio.h>
#include <unistd.h>

struct output { char *buffer; size_t size; size_t count; int fd; };
static void emit(struct output *out, char c) {
    if (out->buffer && out->size && out->count + 1 < out->size) out->buffer[out->count] = c;
    else if (out->fd >= 0) { if (write(out->fd, &c, 1) != 1) return; }
    ++out->count;
}
static void number(struct output *out, unsigned long value, unsigned base, int negative) {
    char digits[sizeof(value) * 8]; size_t n = 0;
    do { unsigned d = (unsigned)(value % base); digits[n++] = (char)(d < 10 ? '0' + d : 'a' + d - 10); value /= base; } while (value);
    if (negative) emit(out, '-');
    while (n) emit(out, digits[--n]);
}
static int format(struct output *out, const char *fmt, va_list ap) {
    size_t initial = out->count;
    while (*fmt) {
        if (*fmt++ != '%') { emit(out, fmt[-1]); continue; }
        if (*fmt == '%') { emit(out, *fmt++); continue; }
        int long_arg = 0;
        if (*fmt == 'l') { long_arg = 1; ++fmt; if (*fmt == 'l') ++fmt; }
        switch (*fmt++) {
        case 's': { const char *s = va_arg(ap, const char *); if (!s) s = "(null)"; while (*s) emit(out, *s++); break; }
        case 'c': emit(out, (char)va_arg(ap, int)); break;
        case 'd': case 'i': { long v = long_arg ? va_arg(ap, long) : va_arg(ap, int); unsigned long mag = v < 0 ? 0ul - (unsigned long)v : (unsigned long)v; number(out, mag, 10, v < 0); break; }
        case 'u': { unsigned long v = long_arg ? va_arg(ap, unsigned long) : va_arg(ap, unsigned); number(out, v, 10, 0); break; }
        case 'x': case 'X': { unsigned long v = long_arg ? va_arg(ap, unsigned long) : va_arg(ap, unsigned); number(out, v, 16, 0); break; }
        case 'p': emit(out, '0'); emit(out, 'x'); number(out, (unsigned long)va_arg(ap, void *), 16, 0); break;
        default: return -1;
        }
    }
    return (int)(out->count - initial);
}
int putchar(int c) { char ch = (char)c; return write(1, &ch, 1) == 1 ? (unsigned char)ch : -1; }
int puts(const char *s) { size_t n=0; while (s[n]) ++n; if (write(1,s,n) != (ssize_t)n || write(1,"\n",1) != 1) return -1; return (int)(n+1); }
int printf(const char *fmt, ...) { struct output out={0}; out.fd=1; va_list ap; va_start(ap,fmt); int r=format(&out,fmt,ap); va_end(ap); return r; }
int snprintf(char *buffer, size_t size, const char *fmt, ...) {
    struct output out={buffer,size,0,-1}; va_list ap; va_start(ap,fmt); int r=format(&out,fmt,ap); va_end(ap);
    if (size) buffer[out.count < size ? out.count : size-1] = 0;
    return r;
}
