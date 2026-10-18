/* spawntest — processos no estilo Unix na personalidade Linux do Pulsar:
 * pipes, fork, vfork, execve, posix_spawn (com redirecionamento) e
 * waitpid. Chama a si mesmo como filho ("spawntest filho ..."). */
#include <errno.h>
#include <fcntl.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

extern char** environ;
static int fails;
static int global_value = 100;

static void check(const char* what, int ok) {
    printf("[spawntest] %s %s\n", ok ? "ok   " : "FALHA", what);
    if (!ok) fails++;
}

static int child_main(int argc, char** argv) {
    /* modos do filho, escolhidos por argv[2] */
    const char* mode = argc > 2 ? argv[2] : "";
    if (strcmp(mode, "eco") == 0) {
        /* escreve no stdout (que o pai redirecionou para uma pipe) */
        for (int i = 3; i < argc; i++) printf("%s%s", argv[i], i + 1 < argc ? " " : "\n");
        fflush(stdout);
        return 0;
    }
    if (strcmp(mode, "codigo") == 0) return atoi(argv[3]);
    if (strcmp(mode, "env") == 0) return getenv("PULSAR_TESTE") && strcmp(getenv("PULSAR_TESTE"), "sim") == 0 ? 11 : 12;
    return 99;
}

int main(int argc, char** argv) {
    if (argc > 1 && strcmp(argv[1], "filho") == 0) return child_main(argc, argv);

    /* 1. pipe no mesmo processo */
    int p[2];
    check("pipe", pipe(p) == 0);
    check("write na pipe", write(p[1], "abc", 3) == 3);
    char b[64] = {0};
    check("read da pipe", read(p[0], b, sizeof b) == 3 && memcmp(b, "abc", 3) == 0);
    close(p[1]);
    check("EOF depois de fechar a escrita", read(p[0], b, sizeof b) == 0);
    close(p[0]);

    /* 2. fork: memoria isolada, codigo de saida, pipe entre processos */
    pipe(p);
    pid_t pid = fork();
    if (pid == 0) {
        global_value = 555;                      /* so no filho */
        close(p[0]);
        struct timespec t = {0, 80 * 1000 * 1000};
        nanosleep(&t, NULL);                     /* o pai ja esta bloqueado no read */
        write(p[1], "ola do filho", 12);
        _exit(7);
    }
    check("fork retorna pid > 0", pid > 0);
    close(p[1]);
    memset(b, 0, sizeof b);
    ssize_t n = read(p[0], b, sizeof b);         /* bloqueia ate o filho escrever */
    check("read bloqueante recebe do filho", n == 12 && strcmp(b, "ola do filho") == 0);
    check("EOF quando o filho sai", read(p[0], b, sizeof b) == 0);
    close(p[0]);
    int st = 0;
    check("waitpid do filho", waitpid(pid, &st, 0) == pid);
    check("codigo de saida 7", WIFEXITED(st) && WEXITSTATUS(st) == 7);
    check("memoria do pai intacta", global_value == 100);
    check("sem mais filhos -> ECHILD", waitpid(-1, &st, 0) == -1 && errno == ECHILD);

    /* 3. posix_spawn com stdout redirecionado para uma pipe */
    pipe(p);
    posix_spawn_file_actions_t fa;
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_adddup2(&fa, p[1], 1);
    posix_spawn_file_actions_addclose(&fa, p[0]);
    char* args[] = {"spawntest", "filho", "eco", "texto", "via", "posix_spawn", NULL};
    int rc = posix_spawn(&pid, "/bin/spawntest", &fa, NULL, args, environ);
    posix_spawn_file_actions_destroy(&fa);
    check("posix_spawn", rc == 0 && pid > 0);
    close(p[1]);
    char out[128] = {0};
    size_t got = 0;
    while ((n = read(p[0], out + got, sizeof out - 1 - got)) > 0) got += n;
    close(p[0]);
    check("saida do filho pela pipe", strcmp(out, "texto via posix_spawn\n") == 0);
    waitpid(pid, &st, 0);
    check("filho do posix_spawn saiu com 0", WIFEXITED(st) && WEXITSTATUS(st) == 0);

    /* 4. posix_spawn de arquivo inexistente: erro chega pelo pipe interno */
    rc = posix_spawn(&pid, "/bin/nao_existe", NULL, NULL, args, environ);
    check("posix_spawn inexistente -> ENOENT", rc == ENOENT);

    /* 5. fork + execve com ambiente */
    pid = fork();
    if (pid == 0) {
        char* a[] = {"spawntest", "filho", "env", NULL};
        char* e[] = {"PULSAR_TESTE=sim", NULL};
        execve("/bin/spawntest", a, e);
        _exit(98);                               /* so se o execve falhar */
    }
    waitpid(pid, &st, 0);
    check("fork + execve (ambiente novo)", WIFEXITED(st) && WEXITSTATUS(st) == 11);

    /* 6. vfork + _exit */
    pid = vfork();
    if (pid == 0) _exit(3);
    waitpid(pid, &st, 0);
    check("vfork + _exit(3)", WIFEXITED(st) && WEXITSTATUS(st) == 3);

    /* 7. close-on-exec: o filho nao herda o descritor */
    int fd = open("/welcome.txt", O_RDONLY | O_CLOEXEC);
    pid = fork();
    if (pid == 0) {
        char fdarg[16];
        snprintf(fdarg, sizeof fdarg, "%d", fd);
        char* a[] = {"spawntest", "filho", "codigo", "0", NULL};
        /* antes do exec o descritor existe; depois nao (o filho so retorna 0) */
        if (fcntl(fd, F_GETFD) != FD_CLOEXEC) _exit(50);
        execv("/bin/spawntest", a);
        _exit(98);
    }
    waitpid(pid, &st, 0);
    check("FD_CLOEXEC marcado e exec ok", WIFEXITED(st) && WEXITSTATUS(st) == 0);
    close(fd);

    /* 8. varios filhos ao mesmo tempo */
    pid_t kids[5];
    for (int i = 0; i < 5; i++) {
        char code[8];
        snprintf(code, sizeof code, "%d", 20 + i);
        char* a[] = {"spawntest", "filho", "codigo", code, NULL};
        posix_spawn(&kids[i], "/bin/spawntest", NULL, NULL, a, environ);
    }
    int sum = 0;
    for (int i = 0; i < 5; i++) { waitpid(kids[i], &st, 0); sum += WEXITSTATUS(st); }
    check("5 filhos em paralelo (20..24)", sum == 20 + 21 + 22 + 23 + 24);

    printf("[spawntest] fim: %d falha(s)\n", fails);
    return fails;
}
