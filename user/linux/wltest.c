/* wltest — isolamento do servico Wayland do Pulsar: um cliente que viola o
 * protocolo so derruba a propria conexao; matar o wlsrv faz o init
 * reinicia-lo, e as conexoes que ja existiam seguem (cada uma tem o seu
 * tradutor). Um wlhello fica rodando o tempo todo como testemunha. */
#define _GNU_SOURCE
#include <errno.h>
#include <poll.h>
#include <signal.h>
#include <spawn.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>

#define PULSAR_LOOKUP_SERVICE 0x5001
#define SVC_WAYLAND 3
extern char** environ;
static int fails;
static void check(const char* what, int ok) {
    printf("[wltest] %s %s\n", ok ? "ok   " : "FALHA", what);
    fflush(stdout);
    if (!ok) fails++;
}

static int wl_connect(void) {
    struct sockaddr_un sa = {.sun_family = AF_UNIX, .sun_path = "/run/wayland-0"};
    int s = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    for (int i = 0; i < 100; i++) {
        if (connect(s, (struct sockaddr*)&sa, sizeof sa) == 0) return s;
        usleep(100000);
    }
    close(s);
    return -1;
}

/* le uma resposta: devolve o opcode do primeiro evento do objeto `obj` (-1: fim) */
static int read_event(int s, uint32_t obj, int ms) {
    uint8_t b[4096];
    size_t len = 0;
    for (;;) {
        struct pollfd pf = {s, POLLIN, 0};
        if (poll(&pf, 1, ms) <= 0) return -2;
        ssize_t n = read(s, b + len, sizeof b - len);
        if (n <= 0) return -1;
        len += n;
        size_t pos = 0;
        while (len - pos >= 8) {
            uint32_t* m = (uint32_t*)(b + pos);
            uint32_t size = m[1] >> 16;
            if (len - pos < size) break;
            if (m[0] == obj) return m[1] & 0xFFFF;
            pos += size;
        }
        memmove(b, b + pos, len - pos);
        len -= pos;
    }
}

/* wl_display.sync(id) -> wl_callback.done: o tradutor responde? */
static int sync_works(int s, uint32_t id) {
    uint32_t msg[] = {1, (12u << 16) | 0, id};
    if (write(s, msg, sizeof msg) != sizeof msg) return 0;
    return read_event(s, id, 3000) == 0;
}

static int registry_works(int s) {
    uint32_t msg[] = {1, (12u << 16) | 1, 2};            /* get_registry(2) */
    write(s, msg, sizeof msg);
    return read_event(s, 2, 3000) == 0;                  /* registry.global */
}

int main(void) {
    /* testemunha: um cliente de verdade, rodando o tempo todo */
    pid_t witness;
    char* wargs[] = {"wlhello", "loop", NULL};
    posix_spawn(&witness, "/bin/wlhello", NULL, NULL, wargs, environ);
    usleep(1500000);
    check("cliente testemunha rodando", kill(witness, 0) == 0);

    /* 1. cliente que viola o protocolo */
    int bad = wl_connect();
    uint32_t garbage[] = {77, (8u << 16) | 3};          /* objeto 77 nao existe */
    write(bad, garbage, sizeof garbage);
    check("cliente invalido recebe wl_display.error", read_event(bad, 1, 3000) == 0);
    check("e e desconectado", read_event(bad, 1, 3000) == -1);
    close(bad);
    check("testemunha intacta", kill(witness, 0) == 0);

    /* 2. uma segunda conexao, que vai sobreviver a queda do wlsrv */
    int c2 = wl_connect();
    check("segundo cliente conecta", c2 >= 0 && registry_works(c2));
    long wl = syscall(PULSAR_LOOKUP_SERVICE, SVC_WAYLAND);
    check("wlsrv registrado como servico", wl > 0);

    /* 3. matar o wlsrv: o init o reinicia; clientes novos conectam */
    kill(wl, SIGKILL);
    long nwl = -1;
    for (int i = 0; i < 100; i++) {
        nwl = syscall(PULSAR_LOOKUP_SERVICE, SVC_WAYLAND);
        if (nwl > 0 && nwl != wl) break;
        usleep(100000);
    }
    check("init reiniciou o wlsrv (pid novo)", nwl > 0 && nwl != wl);
    int c3 = wl_connect();
    check("cliente novo conecta no wlsrv reiniciado", c3 >= 0 && registry_works(c3));
    check("conexao antiga segue viva (o tradutor dela nao depende do wlsrv)", sync_works(c2, 100));
    check("testemunha intacta depois de tudo", kill(witness, 0) == 0);

    kill(witness, SIGTERM);
    int st;
    waitpid(witness, &st, 0);
    printf("[wltest] fim: %d falha(s)\n", fails);
    return fails;
}
