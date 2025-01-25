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

static int execve_hook_debug = -1;

#define DEBUG_PRINT(fmt, ...)                                                            \
    do {                                                                                 \
        if (execve_hook_debug) { fprintf(stderr, "[EXECVE_HOOK] " fmt, ##__VA_ARGS__); } \
    } while (0)

static void SAFE_PRINT(const char *str) {
    if (!execve_hook_debug) return;
    for (; *str; str++) {
        unsigned char c = (unsigned char)*str;
        if ((c >= 32 && c < 127)) { fputc(c, stderr); }
        else { fprintf(stderr, "\\x%02x", c); }
    }
    fprintf(stderr, "\n");
}

static void init_debug(void) {
    fflush(stderr);
    if (execve_hook_debug == -1) {
        execve_hook_debug = (getenv("EXECVE_HOOK_DEBUG") != NULL);
        if (execve_hook_debug) { fprintf(stderr, "[EXECVE_HOOK] Debug mode is enabled.\n"); }
    }
}

typedef int (*real_execve_t)(const char *filename, char *const argv[], char *const envp[]);

void build_new_argv(const char *filename, const size_t argc, char *const argv[], char **new_argv) {
    DEBUG_PRINT("build_new_argv() called. filename=%s, argc=%zu\n", filename, argc);
    new_argv[0] = QEMU_ARM_STATIC_PATH;
    new_argv[1] = (char *)filename;
    for (size_t i = 1; i < argc; ++i) new_argv[i + 1] = argv[i];
    new_argv[argc + 1] = NULL;
    for (int i = 0; new_argv[i] != NULL; ++i) DEBUG_PRINT("  new_argv[%d] = %s\n", i, new_argv[i]);
}

void build_new_envp(const size_t envc, char *const envp[], char **new_envp) {
    DEBUG_PRINT("build_new_envp() called. envc=%zu\n", envc);
    size_t j = 0;
    for (size_t i = 0; i < envc; ++i) {
        if (strncmp(envp[i], "LD_PRELOAD=", 11) == 0) {
            DEBUG_PRINT("  LD_PRELOAD found: ");
            SAFE_PRINT(envp[i]);
            continue;
        }
        new_envp[j++] = (char *)envp[i];
    }
    new_envp[j] = NULL;
    for (int i = 0; new_envp[i] != NULL; ++i) {
        DEBUG_PRINT("  new_envp[%d] = ", i);
        SAFE_PRINT(new_envp[i]);
    }
}

int check_qemu_arm_static(const char *filepath) {
    DEBUG_PRINT("check_qemu_arm_static() called. filepath=%s\n", filepath);
    unsigned char output[EVP_MAX_MD_SIZE];
    FILE *file = fopen(filepath, "rb");
    if (!file) {
        perror("fopen");
        DEBUG_PRINT("check_qemu_arm_static: fopen failed on %s\n", filepath);
        return -1;
    }
    DEBUG_PRINT("check_qemu_arm_static: fopen succeeded.\n");
    EVP_MD_CTX *mdctx = EVP_MD_CTX_new();
    if (mdctx == NULL) {
        perror("EVP_MD_CTX_new");
        fclose(file);
        DEBUG_PRINT("check_qemu_arm_static: EVP_MD_CTX_new failed\n");
        return -1;
    }
    DEBUG_PRINT("check_qemu_arm_static: EVP_MD_CTX_new succeeded.\n");
    if (EVP_DigestInit_ex(mdctx, EVP_sha256(), NULL) != 1) {
        perror("EVP_DigestInit_ex");
        EVP_MD_CTX_free(mdctx);
        fclose(file);
        DEBUG_PRINT("check_qemu_arm_static: EVP_DigestInit_ex failed\n");
        return -1;
    }
    DEBUG_PRINT("check_qemu_arm_static: EVP_DigestInit_ex succeeded. Start reading & updating hash...\n");

    unsigned char buffer[4096];
    size_t bytesRead;
    while ((bytesRead = fread(buffer, 1, sizeof(buffer), file)) > 0) {
        if (EVP_DigestUpdate(mdctx, buffer, bytesRead) != 1) {
            perror("EVP_DigestUpdate");
            EVP_MD_CTX_free(mdctx);
            fclose(file);
            DEBUG_PRINT("check_qemu_arm_static: EVP_DigestUpdate failed\n");
            return -1;
        }
    }
    if (ferror(file)) {
        perror("fread");
        EVP_MD_CTX_free(mdctx);
        fclose(file);
        DEBUG_PRINT("check_qemu_arm_static: ferror detected during fread\n");
        return -1;
    }
    DEBUG_PRINT("check_qemu_arm_static: file read complete.\n");

    unsigned int hash_length;
    if (EVP_DigestFinal_ex(mdctx, output, &hash_length) != 1) {
        perror("EVP_DigestFinal_ex");
        EVP_MD_CTX_free(mdctx);
        fclose(file);
        DEBUG_PRINT("check_qemu_arm_static: EVP_DigestFinal_ex failed\n");
        return -1;
    }
    EVP_MD_CTX_free(mdctx);
    fclose(file);

    char hash_string[SHA256_DIGEST_LENGTH * 2 + 1];
    for (int i = 0; i < SHA256_DIGEST_LENGTH; i++) { sprintf(&hash_string[i * 2], "%02x", output[i]); }
    hash_string[SHA256_DIGEST_LENGTH * 2] = '\0';

    DEBUG_PRINT("check_qemu_arm_static: SHA256 = %s\n", hash_string);

    int cmp_result = strcmp(hash_string, QEMU_ARM_STATIC_HASH);
    DEBUG_PRINT("check_qemu_arm_static: strcmp result=%d (0 means match)\n", cmp_result);

    return cmp_result ? 1 : 0;
}

int check_arm_elf(const char *filepath) {
    DEBUG_PRINT("check_arm_elf() called. filepath=%s\n", filepath);
    int fd = open(filepath, O_RDONLY);
    if (fd < 0) {
        perror("open");
        DEBUG_PRINT("check_arm_elf: open failed on %s\n", filepath);
        return -1;
    }
    DEBUG_PRINT("check_arm_elf: file opened successfully.\n");
    Elf32_Ehdr elf_header;
    ssize_t read_bytes = read(fd, &elf_header, sizeof(Elf32_Ehdr));

    if (read_bytes != sizeof(Elf32_Ehdr)) {
        DEBUG_PRINT("check_arm_elf: not a valid ELF header size. read_bytes=%zd\n", read_bytes);
        close(fd);
        return 1;
    }
    else
        DEBUG_PRINT("check_arm_elf: read ELF header successfully, sizeof(Elf32_Ehdr)=%zu\n", sizeof(Elf32_Ehdr));

    if (memcmp(elf_header.e_ident, ELFMAG, SELFMAG) != 0) {
        DEBUG_PRINT("check_arm_elf: not an ELF file (magic mismatch).\n");
        close(fd);
        return 1;
    }
    else
        DEBUG_PRINT("check_arm_elf: magic matched.\n");

    if (elf_header.e_ident[EI_CLASS] != ELFCLASS32) {
        DEBUG_PRINT("check_arm_elf: not a 32-bit ELF. e_ident[EI_CLASS]=%d\n", elf_header.e_ident[EI_CLASS]);
        close(fd);
        return 1;
    }
    else
        DEBUG_PRINT("check_arm_elf: recognized a 32-bit ELF.\n");

    if (elf_header.e_machine != EM_ARM) {
        DEBUG_PRINT("check_arm_elf: not an ARM ELF. e_machine=%d\n", elf_header.e_machine);
        close(fd);
        return 1;
    }
    else
        DEBUG_PRINT("check_arm_elf: recognized an ARM ELF.\n");

    if (elf_header.e_type != ET_EXEC && elf_header.e_type != ET_DYN) {
        DEBUG_PRINT("check_arm_elf: ELF type is not ET_EXEC or ET_DYN. e_type=%d\n", elf_header.e_type);
        close(fd);
        return 1;
    }
    else
        DEBUG_PRINT("check_arm_elf: recognized an ET_EXEC or ET_DYN ELF.\n");

    DEBUG_PRINT("check_arm_elf: recognized a valid ARM 32-bit ELF.\n");

    close(fd);
    return 0;
}

int get_realpath(const char *filename, char *resolved_path) {
    DEBUG_PRINT("get_realpath() called. filename=%s\n", filename);
    if (access(filename, F_OK) != 0) {
        DEBUG_PRINT("get_realpath: access(%s) failed, file not found.\n", filename);
        return -1;
    }
    if (realpath(filename, resolved_path) == NULL) {
        DEBUG_PRINT("get_realpath: realpath(%s) returned NULL.\n", filename);
        return -1;
    }
    DEBUG_PRINT("get_realpath: resolved path = %s\n", resolved_path);
    return 0;
}

int execve(const char *filename, char *const argv[], char *const envp[]) {
    init_debug();
    static real_execve_t real_execve = NULL;
    DEBUG_PRINT("Hooked execve() called. filename=%s\n", filename);
    DEBUG_PRINT("\n");

    DEBUG_PRINT("Received argv:\n");
    for (size_t i = 0; argv[i] != NULL; ++i) { DEBUG_PRINT("  argv[%zu] = %s\n", i, argv[i]); }
    DEBUG_PRINT("\n");

    DEBUG_PRINT("Received envp:\n");
    for (size_t i = 0; envp[i] != NULL; ++i) {
        DEBUG_PRINT("  envp[%zu] = ", i);
        SAFE_PRINT(envp[i]);
    }
    DEBUG_PRINT("\n");

    DEBUG_PRINT("Checking real_execve starts\n");
    if (!real_execve) {
        DEBUG_PRINT("Loading original execve with dlsym(RTLD_NEXT, \"execve\")...\n");
        real_execve = (real_execve_t)dlsym(RTLD_NEXT, "execve");
        if (!real_execve) {
            fprintf(stderr, "[ERROR] Failed to load original execve: %s\n", dlerror());
            _exit(1);
        }
        DEBUG_PRINT("Original execve loaded successfully.\n");
    }
    DEBUG_PRINT("Checking real_execve ends\n");
    DEBUG_PRINT("\n");

    DEBUG_PRINT("Getting realpath starts\n");
    char filepath[PATH_MAX];
    int invalid_path = get_realpath(filename, filepath);
    DEBUG_PRINT("Getting realpath ends\n");
    DEBUG_PRINT("\n");

    if (invalid_path != 0) {
        DEBUG_PRINT("Invalid path for %s, calling real_execve directly.\n", filename);
        return real_execve(filename, argv, envp);
    }

    DEBUG_PRINT("Checking ARM ELF starts\n");
    int is_arm_elf = check_arm_elf(filepath);
    DEBUG_PRINT("Checking ARM ELF ends\n");
    DEBUG_PRINT("\n");

    DEBUG_PRINT("Checking QEMU ARM static starts\n");
    int is_qemu_arm_static = check_qemu_arm_static(filepath);
    DEBUG_PRINT("Checking QEMU ARM static ends\n");
    DEBUG_PRINT("\n");

    if (invalid_path == 0 && is_arm_elf == 0) {
        size_t argc = 0, envc = 0;
        while (argv[argc] != NULL) ++argc;
        while (envp[envc] != NULL) ++envc;
        char *new_argv[argc + 2], *new_envp[envc + 1];

        DEBUG_PRINT("Building new argv starts\n");
        build_new_argv(filepath, argc, argv, new_argv);
        DEBUG_PRINT("Building new argv ends\n");
        DEBUG_PRINT("\n");

        DEBUG_PRINT("Building new envp starts\n");
        build_new_envp(envc, envp, new_envp);
        DEBUG_PRINT("Building new envp ends\n");
        DEBUG_PRINT("\n");
        DEBUG_PRINT("\n");
        DEBUG_PRINT("\n");

        DEBUG_PRINT("Summary\n");
        DEBUG_PRINT("Ready to call real_execve with new arguments:\n");
        DEBUG_PRINT("  filename=%s\n", QEMU_ARM_STATIC_PATH);
        DEBUG_PRINT("  argv:\n");
        for (size_t i = 0; new_argv[i] != NULL; ++i) { DEBUG_PRINT("    argv[%zu] = %s\n", i, new_argv[i]); }
        DEBUG_PRINT("  envp:\n");
        for (size_t i = 0; new_envp[i] != NULL; ++i) {
            DEBUG_PRINT("    envp[%zu] = ", i);
            SAFE_PRINT(new_envp[i]);
        }
        DEBUG_PRINT("\n");
        DEBUG_PRINT("Done\n");

        return real_execve(QEMU_ARM_STATIC_PATH, new_argv, new_envp);
    }
    if (invalid_path == 0 && is_qemu_arm_static == 0) {
        size_t envc = 0;
        while (envp[envc] != NULL) ++envc;
        char *new_envp[envc + 1];

        DEBUG_PRINT("Building new envp starts\n");
        build_new_envp(envc, envp, new_envp);
        DEBUG_PRINT("Building new envp ends\n");
        DEBUG_PRINT("\n");
        DEBUG_PRINT("\n");
        DEBUG_PRINT("\n");

        DEBUG_PRINT("Summary\n");
        DEBUG_PRINT("Ready to call real_execve with new arguments:\n");
        DEBUG_PRINT("  filename=%s\n", filename);
        DEBUG_PRINT("  argv:\n");
        for (size_t i = 0; argv[i] != NULL; ++i) { DEBUG_PRINT("    argv[%zu] = %s\n", i, argv[i]); }
        DEBUG_PRINT("  envp:\n");
        for (size_t i = 0; new_envp[i] != NULL; ++i) {
            DEBUG_PRINT("    envp[%zu] = ", i);
            SAFE_PRINT(new_envp[i]);
        }
        DEBUG_PRINT("\n");
        DEBUG_PRINT("Done\n");

        return real_execve(filename, argv, new_envp);
    }

    return real_execve(filename, argv, envp);
}

