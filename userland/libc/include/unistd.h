#ifndef CARLIO_UNISTD_H
#define CARLIO_UNISTD_H
#include <stddef.h>
#include <stdint.h>
#include <sys/types.h>
typedef long ssize_t;
typedef long off_t;
int close(int fd);
void _exit(int status) __attribute__((noreturn));
ssize_t read(int fd, void *buffer, size_t count);
ssize_t write(int fd, const void *buffer, size_t count);
long syscall(long number, ...);
pid_t getpid(void);
#endif
