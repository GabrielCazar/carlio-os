#include <stdarg.h>
#include <errno.h>
#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <time.h>
#include <sys/utsname.h>
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
int unlink(const char *path) { return (int)raw6(87,(long)path,0,0,0,0,0); }
int unlinkat(int directory,const char *path,int flags) { return (int)raw6(263,directory,(long)path,flags,0,0,0); }
int rmdir(const char *path) { return (int)raw6(84,(long)path,0,0,0,0,0); }
int chown(const char *path,uid_t owner,gid_t group) { return (int)raw6(92,(long)path,owner,group,0,0,0); }
int lchown(const char *path,uid_t owner,gid_t group) { return (int)raw6(94,(long)path,owner,group,0,0,0); }
int fchown(int fd,uid_t owner,gid_t group) { return (int)raw6(93,fd,owner,group,0,0,0); }
int fchownat(int directory,const char *path,uid_t owner,gid_t group,int flags) { return (int)raw6(260,directory,(long)path,owner,group,flags,0); }
int fsync(int fd) { return (int)raw6(74,fd,0,0,0,0,0); }
int access(const char *path,int mode) { return (int)raw6(21,(long)path,mode,0,0,0,0); }
int mkdir(const char *path,mode_t mode) { return (int)raw6(83,(long)path,mode,0,0,0,0); }
mode_t umask(mode_t mask) { return (mode_t)raw6(95,mask,0,0,0,0,0); }
int chmod(const char *path,mode_t mode) { return (int)raw6(90,(long)path,mode,0,0,0,0); }
int fchmod(int fd,mode_t mode) { return (int)raw6(91,fd,mode,0,0,0,0); }
int fchmodat(int directory,const char *path,mode_t mode,int flags) { return (int)raw6(268,directory,(long)path,mode,flags,0,0); }
char *getcwd(char *buffer,size_t size) { long r=raw6(79,(long)buffer,size,0,0,0,0); return r<0?(char *)0:buffer; }
int fstat(int fd,struct stat *buffer) { return (int)raw6(5,fd,(long)buffer,0,0,0,0); }
int fstatat(int directory,const char *path,struct stat *buffer,int flags) { return (int)raw6(262,directory,(long)path,(long)buffer,flags,0,0); }
int stat(const char *path,struct stat *buffer) { return fstatat(AT_FDCWD,path,buffer,0); }
pid_t getpid(void) { return (pid_t)raw6(39,0,0,0,0,0,0); }
pid_t gettid(void) { return (pid_t)raw6(186,0,0,0,0,0,0); }
uid_t getuid(void) { return (uid_t)raw6(102,0,0,0,0,0,0); }
uid_t geteuid(void) { return (uid_t)raw6(107,0,0,0,0,0,0); }
gid_t getgid(void) { return (gid_t)raw6(104,0,0,0,0,0,0); }
gid_t getegid(void) { return (gid_t)raw6(108,0,0,0,0,0,0); }
int getgroups(int size,gid_t groups[]) { return (int)raw6(115,size,(long)groups,0,0,0,0); }
int brk(void *address) {
    long current=raw6(12,(long)address,0,0,0,0,0);
    if ((void *)current != address) { errno=ENOMEM; return -1; }
    return 0;
}
void *sbrk(long increment) {
    long current=raw6(12,0,0,0,0,0,0);
    if (current == -1) return (void *)-1;
    unsigned long previous=(unsigned long)current;
    unsigned long requested=previous;
    if (increment >= 0) {
        unsigned long amount=(unsigned long)increment;
        if (previous > ~0UL-amount) { errno=ENOMEM; return (void *)-1; }
        requested=previous+amount;
    } else {
        unsigned long amount=0UL-(unsigned long)increment;
        if (previous < amount) { errno=ENOMEM; return (void *)-1; }
        requested=previous-amount;
    }
    long result=raw6(12,(long)requested,0,0,0,0,0);
    if ((unsigned long)result != requested) { errno=ENOMEM; return (void *)-1; }
    return (void *)previous;
}
int uname(struct utsname *name) { return (int)raw6(63,(long)name,0,0,0,0,0); }
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
int mprotect(void *addr,size_t len,int prot) { return (int)raw6(10,(long)addr,len,prot,0,0,0); }
int clock_gettime(clockid_t clock_id,struct timespec *value) { return (int)raw6(228,clock_id,(long)value,0,0,0,0); }
int clock_getres(clockid_t clock_id,struct timespec *resolution) { return (int)raw6(229,clock_id,(long)resolution,0,0,0,0); }
