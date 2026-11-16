#ifndef CARLIO_SYS_MMAN_H
#define CARLIO_SYS_MMAN_H
#include <stddef.h>
#define PROT_NONE 0
#define PROT_READ 1
#define PROT_WRITE 2
#define PROT_EXEC 4
#define MAP_SHARED 1
#define MAP_PRIVATE 2
#define MAP_ANONYMOUS 32
void *mmap(void *address, size_t length, int protection, int flags, int fd, long offset);
int munmap(void *address, size_t length);
#endif
