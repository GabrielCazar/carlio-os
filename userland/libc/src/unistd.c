#include <stdarg.h>
#include <errno.h>
#include <fcntl.h>
#include <sys/mman.h>
#include <unistd.h>

static long raw6(long n, long a, long b, long c, long d, long e, long f) {
    register long r10 __asm__("r10") = d;
    register long r8 __asm__("r8") = e;
    register long r9 __asm__("r9") = f;
    long result;
    __asm__ volatile("syscall" : "=a"(result) : "a"(n), "D"(a), "S"(b), "d"(c), "r"(r10), "r"(r8), "r"(r9) : "rcx", "r11", "memory");
    if ((unsigned long)result >= (unsigned long)-4095) { errno = -result; return -1; }
    return result;
}
long syscall(long n, ...) {
    va_list ap; va_start(ap, n);
    long a=va_arg(ap,long), b=va_arg(ap,long), c=va_arg(ap,long), d=va_arg(ap,long), e=va_arg(ap,long), f=va_arg(ap,long);
    va_end(ap); return raw6(n,a,b,c,d,e,f);
}
ssize_t read(int fd, void *p, size_t n) { return raw6(0,fd,(long)p,n,0,0,0); }
ssize_t write(int fd, const void *p, size_t n) { return raw6(1,fd,(long)p,n,0,0,0); }
int close(int fd) { return (int)raw6(3,fd,0,0,0,0,0); }
pid_t getpid(void) { return (pid_t)raw6(39,0,0,0,0,0,0); }
void _exit(int status) { raw6(60,status,0,0,0,0,0); for (;;) __asm__ volatile("pause"); }
int openat(int dirfd, const char *path, int flags, ...) {
    mode_t mode=0; if (flags & O_CREAT) { va_list ap; va_start(ap,flags); mode=va_arg(ap,int); va_end(ap); }
    return (int)raw6(257,dirfd,(long)path,flags,mode,0,0);
}
int open(const char *path, int flags, ...) {
    mode_t mode=0; if (flags & O_CREAT) { va_list ap; va_start(ap,flags); mode=va_arg(ap,int); va_end(ap); }
    return openat(-100,path,flags,mode);
}
void *mmap(void *addr,size_t len,int prot,int flags,int fd,long off) { long r=raw6(9,(long)addr,len,prot,flags,fd,off); return r<0?(void *)-1:(void *)r; }
int munmap(void *addr,size_t len) { return (int)raw6(11,(long)addr,len,0,0,0,0); }
