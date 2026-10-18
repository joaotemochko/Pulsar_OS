/* wlsrv — servico de compatibilidade Wayland do Pulsar OS (porteiro).
 *
 * Programas graficos Linux falam Wayland; o Pulsar tem o seu proprio
 * gerenciador de janelas e protocolo. Este servico so escuta em
 * /run/wayland-0 e, para cada programa que conecta, inicia um processo
 * wlclient dedicado (a conexao vai no descritor 3), que traduz entre os
 * dois. Assim um cliente problematico derruba apenas o proprio tradutor,
 * nunca este servico, o WM ou os outros programas. O init o supervisiona. */
#include "pulsar.h"
#include <string.h>

#define SOCK_PATH "/run/wayland-0"

int main(void) {
    sys_mkdir("/run", 0755);
    sys_unlink(SOCK_PATH);                    /* sobra de uma execucao anterior */
    i64 s = sys_socket(P_AF_UNIX, P_SOCK_STREAM | P_O_CLOEXEC);
    PSockaddrUn a; memset(&a, 0, sizeof a);
    a.family = P_AF_UNIX;
    strcpy(a.path, SOCK_PATH);
    if (s < 0 || sys_bind(s, &a, sizeof a) < 0 || sys_listen(s, 16) < 0) {
        puts_("[wlsrv] nao foi possivel escutar em " SOCK_PATH "\n");
        return 1;
    }
    /* o descritor 3 e o da conexao entregue a cada tradutor: tira o
     * socket de escuta do caminho */
    i64 hi = sys_dup(s, (i64)(16ULL << 32));
    sys_close(s);
    s = hi;
    sys_fcntl(s, 2, 1);                       /* F_SETFD: close-on-exec */
    sys_register(SVC_WAYLAND);
    puts_("[wlsrv] servico Wayland pronto em " SOCK_PATH "\n");
    static const u8 args[] = { 1, 0, 0, 0, 0, 0, 0, 0, 'w', 'l', 'c', 'l', 'i', 'e', 'n', 't', 0 };
    for (;;) {
        i64 c = sys_accept(s, P_O_CLOEXEC);
        if (c < 0) { sys_sleep(10); continue; }
        /* o tradutor herda a conexao no descritor 3 (sem close-on-exec) */
        if (c != 3) { sys_dup(c, 3); sys_close(c); }
        else sys_fcntl(3, 2, 0);
        i64 pid = sys_spawn_ex("/wlclient.pulse", args, sizeof args);
        sys_close(3);
        if (pid < 0) puts_("[wlsrv] falha ao iniciar wlclient\n");
        /* colhe tradutores que ja terminaram */
        i32 st;
        while (sys_wait(-1, &st, 1) > 0) {}
    }
}
