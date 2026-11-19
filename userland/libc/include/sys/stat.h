#ifndef CARLIO_SYS_STAT_H
#define CARLIO_SYS_STAT_H
#include <sys/types.h>
#include <time.h>

#define S_IFMT  0170000
#define S_IFDIR 0040000
#define S_IFREG 0100000
#define AT_FDCWD -100
#define AT_REMOVEDIR 0x200
#define S_ISDIR(mode) (((mode) & S_IFMT) == S_IFDIR)
#define S_ISREG(mode) (((mode) & S_IFMT) == S_IFREG)

struct stat {
    dev_t st_dev;
    ino_t st_ino;
    nlink_t st_nlink;
    mode_t st_mode;
    uid_t st_uid;
    gid_t st_gid;
    int __pad0;
    dev_t st_rdev;
    off_t st_size;
    long st_blksize;
    long st_blocks;
    struct timespec st_atim;
    struct timespec st_mtim;
    struct timespec st_ctim;
    long __reserved[3];
};

#define st_atime st_atim.tv_sec
#define st_mtime st_mtim.tv_sec
#define st_ctime st_ctim.tv_sec

int fstat(int descriptor, struct stat *buffer);
int fstatat(int directory, const char *path, struct stat *buffer, int flags);
int stat(const char *path, struct stat *buffer);
int mkdir(const char *path, mode_t mode);
mode_t umask(mode_t mask);
int chmod(const char *path, mode_t mode);
int fchmod(int descriptor, mode_t mode);
int fchmodat(int directory, const char *path, mode_t mode, int flags);
#endif
