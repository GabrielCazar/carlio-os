CC ?= x86_64-elf-gcc
AR ?= x86_64-elf-ar
CFLAGS ?= -std=c11 -ffreestanding -fno-stack-protector -fPIC -Wall -Wextra -Werror -Iinclude
OBJ = build/unistd.o build/errno.o build/string.o build/stdio.o

all: build/libcarlio.a
build:
	mkdir -p build
build/%.o: src/%.c | build
	$(CC) $(CFLAGS) -c $< -o $@
build/libcarlio.a: $(OBJ)
	$(AR) rcs $@ $^
clean:
	rm -rf build
.PHONY: all clean
