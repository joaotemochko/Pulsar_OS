/* unixtest — IPC local na personalidade Linux do Pulsar: sockets AF_UNIX
 * (socketpair SEQPACKET, caminho, nome abstrato, listen/accept, passagem de
 * descritores com SCM_RIGHTS), TCP de loopback (127.0.0.1), memoria
 * compartilhada (memfd, MAP_SHARED anonimo, shm_open) e /proc/self. */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <poll.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/sysinfo.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>

static int fails;
static void check(const char* what, int ok) {
    printf("[unixtest] %s %s\n", ok ? "ok   " : "FALHA", what);
    if (!ok) fails++;
}

static void upcase_echo(int c) {
    char b[64];
    int n = read(c, b, sizeof b);
    for (int i = 0; i < n; i++) b[i] = toupper((unsigned char)b[i]);
    write(c, b, n);
}

/* envia descritores pelo socket */
static int send_fds(int s, int* fds, int n) {
    char data = 'F';
    struct iovec io = {.iov_base = &data, .iov_len = 1};
    char ctl[CMSG_SPACE(sizeof(int) * 4)];
    memset(ctl, 0, sizeof ctl);
    struct msghdr m = {.msg_iov = &io, .msg_iovlen = 1, .msg_control = ctl, .msg_controllen = CMSG_SPACE(sizeof(int) * n)};
    struct cmsghdr* c = CMSG_FIRSTHDR(&m);
    c->cmsg_level = SOL_SOCKET;
    c->cmsg_type = SCM_RIGHTS;
    c->cmsg_len = CMSG_LEN(sizeof(int) * n);
    memcpy(CMSG_DATA(c), fds, sizeof(int) * n);
    return sendmsg(s, &m, 0);
}

static int recv_fds(int s, int* fds, int max) {
    char data;
    struct iovec io = {.iov_base = &data, .iov_len = 1};
    char ctl[CMSG_SPACE(sizeof(int) * 4)];
    struct msghdr m = {.msg_iov = &io, .msg_iovlen = 1, .msg_control = ctl, .msg_controllen = sizeof ctl};
    if (recvmsg(s, &m, 0) != 1) return -1;
    int n = 0;
    for (struct cmsghdr* c = CMSG_FIRSTHDR(&m); c; c = CMSG_NXTHDR(&m, c)) {
        if (c->cmsg_level == SOL_SOCKET && c->cmsg_type == SCM_RIGHTS) {
            n = (c->cmsg_len - CMSG_LEN(0)) / sizeof(int);
            if (n > max) n = max;
            memcpy(fds, CMSG_DATA(c), sizeof(int) * n);
        }
    }
    return n;
}

static long freeram(void) { struct sysinfo si; sysinfo(&si); return si.freeram / 1024; }

int main(int argc, char** argv) {
    int sv[2], st;
    char b[128];

    /* 1. SEQPACKET preserva as mensagens */
    check("socketpair(SOCK_SEQPACKET)", socketpair(AF_UNIX, SOCK_SEQPACKET, 0, sv) == 0);
    write(sv[0], "a", 1); write(sv[0], "bc", 2); write(sv[0], "def", 3);
    int n1 = read(sv[1], b, sizeof b), n2 = read(sv[1], b, sizeof b), n3 = read(sv[1], b, sizeof b);
    check("tres mensagens, tres leituras (1, 2, 3)", n1 == 1 && n2 == 2 && n3 == 3 && memcmp(b, "def", 3) == 0);
    close(sv[0]); close(sv[1]);

    /* 2. socket com caminho: listen/accept entre processos */
    unlink("/tmp/pulsar.sock");
    int srv = socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un un = {.sun_family = AF_UNIX};
    strcpy(un.sun_path, "/tmp/pulsar.sock");
    check("bind(/tmp/pulsar.sock)", bind(srv, (struct sockaddr*)&un, sizeof un) == 0);
    struct stat ss;
    check("o caminho vira um socket (S_ISSOCK)", stat("/tmp/pulsar.sock", &ss) == 0 && S_ISSOCK(ss.st_mode));
    int dup_s = socket(AF_UNIX, SOCK_STREAM, 0);
    check("bind repetido -> EADDRINUSE", bind(dup_s, (struct sockaddr*)&un, sizeof un) == -1 && errno == EADDRINUSE);
    close(dup_s);
    check("listen", listen(srv, 8) == 0);
    pid_t pid = fork();
    if (pid == 0) {
        int c = socket(AF_UNIX, SOCK_STREAM, 0);
        if (connect(c, (struct sockaddr*)&un, sizeof un) != 0) _exit(1);
        write(c, "ola servidor", 12);
        char r[32] = {0};
        read(c, r, sizeof r);
        _exit(strcmp(r, "OLA SERVIDOR") == 0 ? 0 : 2);
    }
    struct pollfd pf = {.fd = srv, .events = POLLIN};
    check("poll no listener ve a conexao", poll(&pf, 1, 3000) == 1 && (pf.revents & POLLIN));
    int c = accept(srv, NULL, NULL);
    check("accept", c >= 0);
    upcase_echo(c);
    waitpid(pid, &st, 0);
    check("cliente recebeu o eco", WIFEXITED(st) && WEXITSTATUS(st) == 0);
    close(c); close(srv);
    check("unlink do socket", unlink("/tmp/pulsar.sock") == 0);

    /* 3. nome abstrato */
    srv = socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un ab = {.sun_family = AF_UNIX};
    memcpy(ab.sun_path, "\0pulsar-abstrato", 16);
    socklen_t alen = offsetof(struct sockaddr_un, sun_path) + 16;
    check("bind abstrato + listen", bind(srv, (struct sockaddr*)&ab, alen) == 0 && listen(srv, 4) == 0);
    int cl = socket(AF_UNIX, SOCK_STREAM, 0);
    check("connect abstrato", connect(cl, (struct sockaddr*)&ab, alen) == 0);
    c = accept(srv, NULL, NULL);
    write(cl, "x", 1);
    check("dados pelo socket abstrato", read(c, b, 1) == 1 && b[0] == 'x');
    close(c); close(cl); close(srv);

    /* 4. SCM_RIGHTS: uma pipe e um arquivo passam para o filho */
    socketpair(AF_UNIX, SOCK_STREAM, 0, sv);
    pid = fork();
    if (pid == 0) {
        close(sv[0]);
        int got[2];
        if (recv_fds(sv[1], got, 2) != 2) _exit(1);
        char w[8] = {0};
        read(got[1], w, 7);                          /* arquivo recebido */
        dprintf(got[0], "recebi:%s", w);             /* pipe recebida */
        _exit(0);
    }
    close(sv[1]);
    int p[2];
    pipe(p);
    int f = open("/welcome.txt", O_RDONLY);
    int pass[2] = {p[1], f};
    check("sendmsg com SCM_RIGHTS", send_fds(sv[0], pass, 2) == 1);
    close(p[1]); close(f);
    memset(b, 0, sizeof b);
    int got = read(p[0], b, sizeof b - 1);
    char expect[64];
    int wf = open("/welcome.txt", O_RDONLY);
    char head[8] = {0};
    read(wf, head, 7);
    close(wf);
    snprintf(expect, sizeof expect, "recebi:%s", head);
    check("filho usou a pipe e o arquivo recebidos", got > 0 && strcmp(b, expect) == 0);
    waitpid(pid, NULL, 0);
    close(p[0]); close(sv[0]);

    /* 5. TCP de loopback */
    srv = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in in = {.sin_family = AF_INET, .sin_port = htons(5555), .sin_addr.s_addr = htonl(INADDR_LOOPBACK)};
    check("bind + listen 127.0.0.1:5555", bind(srv, (struct sockaddr*)&in, sizeof in) == 0 && listen(srv, 4) == 0);
    pid = fork();
    if (pid == 0) {
        int k = socket(AF_INET, SOCK_STREAM, 0);
        if (connect(k, (struct sockaddr*)&in, sizeof in) != 0) _exit(1);
        write(k, "tcp local", 9);
        char r[16] = {0};
        read(k, r, sizeof r);
        _exit(strcmp(r, "TCP LOCAL") == 0 ? 0 : 2);
    }
    struct sockaddr_in peer;
    socklen_t pl = sizeof peer;
    c = accept(srv, (struct sockaddr*)&peer, &pl);
    check("accept TCP local (peer 127.0.0.1)", c >= 0 && peer.sin_addr.s_addr == htonl(INADDR_LOOPBACK));
    upcase_echo(c);
    waitpid(pid, &st, 0);
    check("eco TCP local", WIFEXITED(st) && WEXITSTATUS(st) == 0);
    close(c); close(srv);

    /* 6. memfd + MAP_SHARED entre processos */
    long free0 = freeram();
    int mf = memfd_create("teste", MFD_CLOEXEC);
    check("memfd_create + ftruncate(8 MiB)", mf >= 0 && ftruncate(mf, 8 << 20) == 0);
    char* sh = mmap(NULL, 8 << 20, PROT_READ | PROT_WRITE, MAP_SHARED, mf, 0);
    check("mmap MAP_SHARED do memfd", sh != MAP_FAILED);
    memset(sh, 0, 8 << 20);
    pid = fork();
    if (pid == 0) { strcpy(sh + (4 << 20), "escrito pelo filho"); _exit(0); }
    waitpid(pid, NULL, 0);
    check("pai ve o que o filho escreveu", strcmp(sh + (4 << 20), "escrito pelo filho") == 0);
    char via_read[19] = {0};
    pread(mf, via_read, 18, 4 << 20);
    check("read() do memfd ve o mesmo conteudo", strcmp(via_read, "escrito pelo filho") == 0);
    munmap(sh, 8 << 20);
    close(mf);
    long free1 = freeram();
    check("memoria devolvida ao fechar e desmapear", free0 - free1 < 1024);

    /* 7. MAP_SHARED | MAP_ANONYMOUS */
    atomic_int* cnt = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANONYMOUS, -1, 0);
    *cnt = 0;
    for (int i = 0; i < 4; i++) if (fork() == 0) { for (int k = 0; k < 1000; k++) atomic_fetch_add(cnt, 1); _exit(0); }
    for (int i = 0; i < 4; i++) wait(NULL);
    check("contador compartilhado por 4 filhos = 4000", *cnt == 4000);

    /* 8. shm_open / shm_unlink */
    int so = shm_open("/pulsar-shm", O_CREAT | O_EXCL | O_RDWR, 0600);
    check("shm_open(O_CREAT|O_EXCL)", so >= 0 && ftruncate(so, 4096) == 0);
    char* sm = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, so, 0);
    strcpy(sm, "via shm_open");
    pid = fork();
    if (pid == 0) {
        int o = shm_open("/pulsar-shm", O_RDWR, 0);
        char* m = mmap(NULL, 4096, PROT_READ, MAP_SHARED, o, 0);
        _exit(o >= 0 && m != MAP_FAILED && strcmp(m, "via shm_open") == 0 ? 0 : 1);
    }
    waitpid(pid, &st, 0);
    check("outro processo abre pelo nome", WIFEXITED(st) && WEXITSTATUS(st) == 0);
    check("shm_unlink", shm_unlink("/pulsar-shm") == 0);
    check("depois do unlink: ENOENT", shm_open("/pulsar-shm", O_RDWR, 0) == -1 && errno == ENOENT);
    check("mapeamento continua valido", strcmp(sm, "via shm_open") == 0);
    munmap(sm, 4096);
    close(so);

    /* 9. /proc/self */
    int cf = open("/proc/self/cmdline", O_RDONLY);
    int cn = read(cf, b, sizeof b);
    close(cf);
    check("/proc/self/cmdline = argv", argc == 2 && cn == (int)(strlen(argv[0]) + strlen(argv[1]) + 2)
          && strcmp(b, argv[0]) == 0 && strcmp(b + strlen(argv[0]) + 1, argv[1]) == 0);
    static char sb[1024];
    int sf = open("/proc/self/status", O_RDONLY);
    cn = read(sf, sb, sizeof sb - 1);
    sb[cn > 0 ? cn : 0] = 0;
    close(sf);
    check("/proc/self/status", strstr(sb, "Name:\tunixtest") && strstr(sb, "Threads:\t1"));

    printf("[unixtest] fim: %d falha(s)\n", fails);
    return fails;
}
