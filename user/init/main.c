/* init — supervisor de servicos do Pulsar OS (primeiro processo).
 *
 * Num microkernel, sistema de arquivos, gerenciador de janelas e servicos
 * de compatibilidade sao processos comuns: se um deles cair (bug, falha do
 * kernel atendendo ele), o resto do sistema continua, e o init o inicia de
 * novo. Le /etc/services, inicia cada servico em ordem e fica esperando
 * filhos terminarem (SYS_WAIT). Um servico que cai mais de MAX_BURST vezes
 * em BURST_MS e abandonado, com aviso na serial. */
#include "pulsar.h"
#include <string.h>

#define MAX_SVC   16
#define MAX_BURST 5
#define BURST_MS  30000

typedef struct {
    char line[128];      /* caminho e argumentos */
    i64 pid;
    u64 starts[MAX_BURST];
    int nstarts;
    int dead;            /* abandonado */
} Service;

static Service svc[MAX_SVC];
static int nsvc;

static void log_(const char* a, const char* b, const char* c) {
    char m[192]; char* p = fmt_s(m, "[init] "); p = fmt_s(p, a); p = fmt_s(p, b); p = fmt_s(p, c); fmt_s(p, "\n");
    puts_(m);
}

/* inicia o servico i (argv a partir das palavras da linha) */
static void start(int i) {
    Service* s = &svc[i];
    static u8 block[512];
    int n = 8, argc = 0;
    const char* c = s->line;
    char path[128]; int pl = 0;
    while (*c) {
        while (*c == ' ') c++;
        if (!*c) break;
        const char* w = c;
        while (*c && *c != ' ') c++;
        int len = (int)(c - w);
        if (argc == 0) {
            memcpy(path, w, len); path[len] = 0; pl = len;
            /* argv[0] = nome sem diretorio */
            const char* b = w + len; while (b > w && b[-1] != '/') b--;
            int bl = (int)(w + len - b);
            memcpy(block + n, b, bl); n += bl; block[n++] = 0;
        } else {
            memcpy(block + n, w, len); n += len; block[n++] = 0;
        }
        argc++;
        if (n > 480) break;
    }
    if (pl == 0) return;
    *(u32*)block = (u32)argc;
    *(u32*)(block + 4) = 0;
    u64 now = sys_time_ms();
    /* janela de reinicios: esquece os antigos */
    int k = 0;
    for (int j = 0; j < s->nstarts; j++) if (now - s->starts[j] < BURST_MS) s->starts[k++] = s->starts[j];
    s->nstarts = k;
    if (s->nstarts >= MAX_BURST) {
        s->dead = 1;
        log_("'", path, "' caiu demais; desistindo dele");
        return;
    }
    s->starts[s->nstarts++] = now;
    s->pid = sys_spawn_ex(path, block, (u64)n);
    if (s->pid < 0) { s->dead = 1; log_("nao foi possivel iniciar '", path, "'"); }
}

static void load(void) {
    static char buf[2048];
    i64 fd = sys_open("/etc/services", P_O_RDONLY, 0);
    i64 n = fd >= 0 ? sys_read(fd, buf, sizeof buf - 1) : -1;
    if (fd >= 0) sys_close(fd);
    if (n <= 0) {
        /* sem arquivo: o minimo para ter um desktop */
        strcpy(svc[0].line, "/fsd.pulse");
        strcpy(svc[1].line, "/shell.pulse");
        nsvc = 2;
        return;
    }
    buf[n] = 0;
    char* p = buf;
    while (*p && nsvc < MAX_SVC) {
        char* e = p; while (*e && *e != '\n') e++;
        char save = *e; *e = 0;
        while (*p == ' ' || *p == '\t') p++;
        if (*p && *p != '#') { strncpy(svc[nsvc].line, p, sizeof svc[0].line - 1); nsvc++; }
        *e = save;
        p = *e ? e + 1 : e;
    }
}

int main(void) {
    load();
    for (int i = 0; i < nsvc; i++) {
        start(i);
        sys_sleep(20);                    /* ordem: fsd registra o servico antes do WM */
    }
    for (;;) {
        i32 st = 0;
        i64 pid = sys_wait(-1, &st, 0);
        if (pid < 0) { sys_sleep(1000); continue; }   /* nenhum filho vivo */
        for (int i = 0; i < nsvc; i++) {
            if (svc[i].pid != pid || svc[i].dead) continue;
            char why[48]; char* q;
            if (st & 0x7F) { q = fmt_s(why, "' caiu (sinal "); q = fmt_u(q, (u64)(st & 0x7F)); fmt_s(q, "); reiniciando"); }
            else { q = fmt_s(why, "' terminou (codigo "); q = fmt_u(q, (u64)((st >> 8) & 0xFF)); fmt_s(q, "); reiniciando"); }
            log_("'", svc[i].line, why);
            sys_sleep(200);
            start(i);
        }
    }
    return 0;
}
