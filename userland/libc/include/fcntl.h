#ifndef CARLIO_FCNTL_H
#define CARLIO_FCNTL_H
#include <sys/types.h>
#define O_RDONLY 0
#define O_WRONLY 1
#define O_RDWR 2
#define O_CREAT 0100
#define O_EXCL 0200
#define O_TRUNC 01000
#define O_APPEND 02000
#define AT_REMOVEDIR 0x200
#define AT_SYMLINK_NOFOLLOW 0x100
int open(const char *path, int flags, ...);
int openat(int dirfd, const char *path, int flags, ...);
#endif
