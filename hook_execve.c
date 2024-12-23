#define _GNU_SOURCE
#include <dlfcn.h>
#include <elf.h>
#include <fcntl.h>
#include <limits.h>
#include <openssl/evp.h>
#include <openssl/sha.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

#define QEMU_ARM_STATIC_PATH "/usr/bin/qemu-arm-static"
#define QEMU_ARM_STATIC_HASH "PLACEHOLDER_HASH"

typedef int (*real_execve_t)(const char *filename, char *const argv[], char *const envp[]);

void build_new_argv(const char *filename, const size_t argc, char *const argv[], char **new_argv) {
    new_argv[0] = QEMU_ARM_STATIC_PATH;
    new_argv[1] = (char*)filename;
    for (size_t i = 1; i < argc; ++i) new_argv[i + 1] = argv[i];
    new_argv[argc + 1] = NULL;
}

void build_new_envp(const size_t envc, char *const envp[], char **new_envp) {
    size_t j = 0;
    for (size_t i = 0; i < envc; ++i) {
        if (strncmp(envp[i], "LD_PRELOAD=", 11) == 0) continue;
        new_envp[j++] = (char*)envp[i];
    }
    new_envp[j] = NULL;
}

int check_qemu_arm_static(const char *filepath) {
    unsigned char output[EVP_MAX_MD_SIZE];
    FILE *file = fopen(filepath, "rb");
    if (!file) {
        perror("fopen");
        return -1;
    }
    EVP_MD_CTX *mdctx = EVP_MD_CTX_new();
    if (mdctx == NULL) {
        perror("EVP_MD_CTX_new");
        fclose(file);
        return -1;
    }
    if (EVP_DigestInit_ex(mdctx, EVP_sha256(), NULL) != 1) {
        perror("EVP_DigestInit_ex");
        EVP_MD_CTX_free(mdctx);
        fclose(file);
        return -1;
    }
    unsigned char buffer[4096];
    size_t bytesRead;
    while ((bytesRead = fread(buffer, 1, sizeof(buffer), file)) > 0) {
        if (EVP_DigestUpdate(mdctx, buffer, bytesRead) != 1) {
            perror("EVP_DigestUpdate");
            EVP_MD_CTX_free(mdctx);
            fclose(file);
            return -1;
        }
    }
    if (ferror(file)) {
        perror("fread");
        EVP_MD_CTX_free(mdctx);
        fclose(file);
        return -1;
    }
    unsigned int hash_length;
    if (EVP_DigestFinal_ex(mdctx, output, &hash_length) != 1) {
        perror("EVP_DigestFinal_ex");
        EVP_MD_CTX_free(mdctx);
        fclose(file);
        return -1;
    }
    EVP_MD_CTX_free(mdctx);
    fclose(file);
    char hash_string[SHA256_DIGEST_LENGTH * 2 + 1];
    for (int i = 0; i < SHA256_DIGEST_LENGTH; i++) { sprintf(&hash_string[i * 2], "%02x", output[i]); }
    return strcmp(hash_string, QEMU_ARM_STATIC_HASH) ? 1 : 0;
}

int check_arm_elf(const char *filepath) {
    int fd = open(filepath, O_RDONLY);
    if (fd < 0) {
        perror("open");
        return -1;
    }
    Elf32_Ehdr elf_header;
    ssize_t read_bytes = read(fd, &elf_header, sizeof(Elf32_Ehdr));
    if ((read_bytes != sizeof(Elf32_Ehdr)) || (memcmp(elf_header.e_ident, ELFMAG, SELFMAG) != 0) || (elf_header.e_ident[EI_CLASS] != ELFCLASS32) || (elf_header.e_machine != EM_ARM) ||
        (elf_header.e_type != ET_EXEC && elf_header.e_type != ET_DYN)) {
        close(fd);
        return 1;
    }
    close(fd);
    return 0;
}

int get_realpath(const char *filename, char *resolved_path) {
    if (access(filename, F_OK) != 0) return -1;
    if (realpath(filename, resolved_path) == NULL) return -1;
    return 0;
}

int execve(const char *filename, char *const argv[], char *const envp[]) {
    static real_execve_t real_execve = NULL;

    if (!real_execve) {
        real_execve = (real_execve_t)dlsym(RTLD_NEXT, "execve");
        if (!real_execve) {
            fprintf(stderr, "[ERROR] Failed to load original execve: %s\n", dlerror());
            _exit(1);
        }
    }

    char filepath[PATH_MAX];
    int invalid_path = get_realpath(filename, filepath);

    if (invalid_path == 0 && check_arm_elf(filepath) == 0) {
        size_t argc = 0, envc = 0;
        while (argv[argc] != NULL) ++argc;
        while (envp[envc] != NULL) ++envc;
        char *new_argv[argc + 2], *new_envp[envc + 1];
        build_new_argv(filename, argc, argv, new_argv);
        build_new_envp(envc, envp, new_envp);
        return real_execve(QEMU_ARM_STATIC_PATH, new_argv, new_envp);
    }
    if (invalid_path == 0 && check_qemu_arm_static(filepath) == 0) {
        size_t envc = 0;
        while (envp[envc] != NULL) ++envc;
        char *new_envp[envc + 1];
        build_new_envp(envc, envp, new_envp);
        return real_execve(filename, argv, new_envp);
    }

    return real_execve(filename, argv, envp);
}

