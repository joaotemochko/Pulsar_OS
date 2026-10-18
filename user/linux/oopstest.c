/* oopstest — recuperacao de falhas do kernel ("oops"): provoca, em
 * processos filhos, um panic, um acesso invalido dentro do kernel e um
 * panic com a trava do sistema de arquivos presa (extensao do Pulsar
 * 0x5000). So o filho deve morrer (como num SIGSEGV); o pai e o resto do
 * sistema seguem, e o disco continua utilizavel. */
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

#define PULSAR_DEBUG_OOPS 0x5000

static int fails;
static void check(const char* what, int ok) {
    printf("[oopstest] %s %s\n", ok ? "ok   " : "FALHA", what);
    fflush(stdout);
    if (!ok) fails++;
}

static int fs_works(int round) {
    char path[64], msg[32], back[32] = {0};
    snprintf(path, sizeof path, "/tmp/oops_%d.txt", round);
    snprintf(msg, sizeof msg, "rodada %d", round);
    int fd = open(path, O_CREAT | O_TRUNC | O_RDWR, 0644);
    if (fd < 0) return 0;
    write(fd, msg, strlen(msg));
    pread(fd, back, sizeof back - 1, 0);
    close(fd);
    unlink(path);
    return strcmp(msg, back) == 0;
}

int main(void) {
    static const char* kinds[] = {"panic no kernel", "acesso invalido no kernel", "panic com a trava FS presa"};
    for (int k = 0; k < 3; k++) {
        pid_t pid = fork();
        if (pid == 0) {
            syscall(PULSAR_DEBUG_OOPS, k);
            _exit(0);                                   /* nao deveria chegar aqui */
        }
        int st = 0;
        waitpid(pid, &st, 0);
        char what[96];
        snprintf(what, sizeof what, "%s: so o filho morre (SIGSEGV)", kinds[k]);
        check(what, WIFSIGNALED(st) && WTERMSIG(st) == SIGSEGV);
        snprintf(what, sizeof what, "%s: sistema de arquivos segue funcionando", kinds[k]);
        check(what, fs_works(k));
    }
    int p[2];
    check("pipes e fork seguem funcionando", pipe(p) == 0 && fork() == 0 ? (write(p[1], "ok", 2), _exit(0), 0) : (read(p[0], (char[4]){0}, 2) == 2));
    wait(NULL);
    printf("[oopstest] fim: %d falha(s)\n", fails);
    return fails;
}
