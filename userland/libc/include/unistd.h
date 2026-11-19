#ifndef CARLIO_UNISTD_H
#define CARLIO_UNISTD_H
#include <stddef.h>
#include <stdint.h>
#include <sys/types.h>
#define F_OK 0
#define X_OK 1
#define W_OK 2
#define R_OK 4
typedef long ssize_t;
typedef long off_t;
int close(int fd);
int unlink(const char *path);
int unlinkat(int directory, const char *path, int flags);
int rmdir(const char *path);
int chown(const char *path, uid_t owner, gid_t group);
int lchown(const char *path, uid_t owner, gid_t group);
int fchown(int descriptor, uid_t owner, gid_t group);
int fchownat(int directory, const char *path, uid_t owner, gid_t group, int flags);
int fsync(int fd);
int access(const char *path, int mode);
char *getcwd(char *buffer, size_t size);
void _exit(int status) __attribute__((noreturn));
ssize_t read(int fd, void *buffer, size_t count);
ssize_t write(int fd, const void *buffer, size_t count);
long syscall(long number, ...);
pid_t getpid(void);
pid_t gettid(void);
uid_t getuid(void);
uid_t geteuid(void);
gid_t getgid(void);
gid_t getegid(void);
int getgroups(int size, gid_t groups[]);
int brk(void *address);
void *sbrk(long increment);
#endif
