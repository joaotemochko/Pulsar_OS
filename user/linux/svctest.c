/* svctest — supervisao de servicos: mata o gerenciador de janelas e o
 * sistema de arquivos (servicos do Pulsar, achados pela extensao 0x5001) e
 * confere que o init os reinicia, com pid novo, enquanto este programa (e o
 * resto do sistema) segue rodando. */
#include <signal.h>
#include <stdio.h>
#include <sys/syscall.h>
#include <stdlib.h>
#include <unistd.h>

#define PULSAR_LOOKUP_SERVICE 0x5001
#define SVC_FS 1
#define SVC_WM 2

static int fails;
static void check(const char* what, int ok) {
    printf("[svctest] %s %s\n", ok ? "ok   " : "FALHA", what);
    fflush(stdout);
    if (!ok) fails++;
}

static long svc(int id) { return syscall(PULSAR_LOOKUP_SERVICE, id); }

static long wait_new(int id, long old) {
    for (int i = 0; i < 100; i++) {           /* ate 10 s */
        long p = svc(id);
        if (p > 0 && p != old) return p;
        usleep(100000);
    }
    return -1;
}

int main(void) {
    static const char* names[] = {"", "sistema de arquivos (fsd)", "gerenciador de janelas"};
    int only_wm = getenv("SO_WM") != NULL;
    for (int id = SVC_WM; id >= (only_wm ? SVC_WM : SVC_FS); id--) {
        long pid = -1;
        for (int i = 0; i < 100 && pid < 0; i++) { pid = svc(id); if (pid < 0) usleep(100000); }
        char what[96];
        snprintf(what, sizeof what, "%s esta rodando (pid %ld)", names[id], pid);
        check(what, pid > 0);
        kill(pid, SIGKILL);
        long np = wait_new(id, pid);
        snprintf(what, sizeof what, "%s reiniciado pelo init (pid %ld)", names[id], np);
        check(what, np > 0 && np != pid);
    }
    printf("[svctest] fim: %d falha(s)\n", fails);
    return fails;
}
