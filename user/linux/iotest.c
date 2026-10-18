/* iotest — E/S da personalidade Linux do Pulsar: poll/select/epoll,
 * eventfd, socketpair, /dev, /proc e rede de verdade (DNS pela musl, TCP
 * bloqueante e nao bloqueante). Os testes de rede pulam se nao houver
 * acesso a internet (argumento "semrede" desliga). */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <netdb.h>
#include <netinet/in.h>
#include <poll.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/epoll.h>
#include <sys/eventfd.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include <arpa/inet.h>

static int fails;
static void check(const char* what, int ok) {
    printf("[iotest] %s %s\n", ok ? "ok   " : "FALHA", what);
    if (!ok) fails++;
}

static long ms_since(struct timespec* t0) {
    struct timespec t1;
    clock_gettime(CLOCK_MONOTONIC, &t1);
    return (t1.tv_sec - t0->tv_sec) * 1000 + (t1.tv_nsec - t0->tv_nsec) / 1000000;
}

static int efd;
static void* bump(void* a) {
    (void)a;
    usleep(50000);
    uint64_t v = 3;
    write(efd, &v, 8);
    return NULL;
}

static int read_all(const char* path, char* buf, int cap) {
    int fd = open(path, O_RDONLY);
    if (fd < 0) return -1;
    int n = 0, r;
    while (n < cap - 1 && (r = read(fd, buf + n, cap - 1 - n)) > 0) n += r;
    buf[n] = 0;
    close(fd);
    return n;
}

int main(int argc, char** argv) {
    int net = !(argc > 1 && strcmp(argv[1], "semrede") == 0);
    struct timespec t0;
    char buf[4096];

    /* 1. poll: timeout real e acordado por outro processo */
    int p[2];
    pipe(p);
    struct pollfd pf = {.fd = p[0], .events = POLLIN};
    clock_gettime(CLOCK_MONOTONIC, &t0);
    int r = poll(&pf, 1, 120);
    long el = ms_since(&t0);
    check("poll sem dados: 0 depois de ~120 ms", r == 0 && el >= 115 && el < 1000);
    pid_t pid = fork();
    if (pid == 0) { usleep(60000); write(p[1], "x", 1); _exit(0); }
    clock_gettime(CLOCK_MONOTONIC, &t0);
    r = poll(&pf, 1, 5000);
    el = ms_since(&t0);
    check("poll acorda com escrita do filho", r == 1 && (pf.revents & POLLIN) && el >= 40 && el < 2000);
    read(p[0], buf, 1);
    waitpid(pid, NULL, 0);

    /* 2. select */
    fd_set rs;
    FD_ZERO(&rs);
    FD_SET(p[0], &rs);
    struct timeval tv = {0, 50000};
    check("select sem dados: 0", select(p[0] + 1, &rs, NULL, NULL, &tv) == 0);
    write(p[1], "y", 1);
    FD_ZERO(&rs);
    FD_SET(p[0], &rs);
    tv.tv_usec = 50000;
    check("select com dados", select(p[0] + 1, &rs, NULL, NULL, &tv) == 1 && FD_ISSET(p[0], &rs));
    read(p[0], buf, 1);

    /* 3. eventfd bloqueante acordado por uma thread */
    efd = eventfd(0, EFD_CLOEXEC);
    pthread_t th;
    pthread_create(&th, NULL, bump, NULL);
    uint64_t v = 0;
    r = read(efd, &v, 8);
    check("eventfd: read bloqueia e recebe 3", r == 8 && v == 3);
    pthread_join(th, NULL);

    /* 4. epoll com pipe + eventfd */
    int ep = epoll_create1(EPOLL_CLOEXEC);
    struct epoll_event ev = {.events = EPOLLIN, .data.u64 = 0xAAAA};
    epoll_ctl(ep, EPOLL_CTL_ADD, p[0], &ev);
    ev.data.u64 = 0xBBBB;
    ev.events = EPOLLIN | EPOLLONESHOT;
    epoll_ctl(ep, EPOLL_CTL_ADD, efd, &ev);
    struct epoll_event out[4];
    check("epoll_wait sem eventos: 0", epoll_wait(ep, out, 4, 30) == 0);
    v = 1;
    write(efd, &v, 8);
    write(p[1], "z", 1);
    r = epoll_wait(ep, out, 4, 1000);
    int seen_a = 0, seen_b = 0;
    for (int i = 0; i < r; i++) { seen_a |= out[i].data.u64 == 0xAAAA; seen_b |= out[i].data.u64 == 0xBBBB; }
    check("epoll_wait ve pipe e eventfd (data certo)", r == 2 && seen_a && seen_b);
    read(p[0], buf, 1);
    r = epoll_wait(ep, out, 4, 30);
    check("EPOLLONESHOT nao repete", r == 0);
    close(ep);

    /* 5. socketpair entre processos */
    int sv[2];
    check("socketpair", socketpair(AF_UNIX, SOCK_STREAM, 0, sv) == 0);
    pid = fork();
    if (pid == 0) {
        close(sv[0]);
        char b[16] = {0};
        read(sv[1], b, sizeof b);
        for (char* c = b; *c; c++) *c -= 32;               /* maiusculas */
        write(sv[1], b, strlen(b));
        _exit(0);
    }
    close(sv[1]);
    write(sv[0], "pulsar", 6);
    memset(buf, 0, 16);
    read(sv[0], buf, 16);
    check("socketpair ida e volta (PULSAR)", strcmp(buf, "PULSAR") == 0);
    waitpid(pid, NULL, 0);
    close(sv[0]);

    /* 6. /dev e /proc */
    int fd = open("/dev/null", O_WRONLY);
    check("/dev/null aceita escrita", fd >= 0 && write(fd, "abc", 3) == 3);
    close(fd);
    fd = open("/dev/zero", O_RDONLY);
    memset(buf, 1, 64);
    check("/dev/zero", read(fd, buf, 64) == 64 && buf[0] == 0 && buf[63] == 0);
    close(fd);
    fd = open("/dev/urandom", O_RDONLY);
    uint64_t a = 0, b = 0;
    read(fd, &a, 8);
    read(fd, &b, 8);
    check("/dev/urandom", a != b);
    close(fd);
    char exe[256] = {0};
    readlink("/proc/self/exe", exe, sizeof exe - 1);
    check("/proc/self/exe", strcmp(exe, "/bin/iotest") == 0 || strcmp(exe, "/bin/iotest.pulse") == 0);
    check("/proc/cpuinfo", read_all("/proc/cpuinfo", buf, sizeof buf) > 0 && strstr(buf, "processor"));
    check("/proc/meminfo", read_all("/proc/meminfo", buf, sizeof buf) > 0 && strstr(buf, "MemTotal"));
    check("/proc/self/maps tem [stack]", read_all("/proc/self/maps", buf, sizeof buf) > 0 && strstr(buf, "[stack]"));

    if (net) {
        /* 7. DNS pela musl (UDP para 10.0.2.3) */
        struct addrinfo hints = {.ai_family = AF_INET, .ai_socktype = SOCK_STREAM}, *res = NULL;
        clock_gettime(CLOCK_MONOTONIC, &t0);
        r = getaddrinfo("example.com", "80", &hints, &res);
        check("getaddrinfo(example.com)", r == 0 && res != NULL);
        if (r == 0) {
            char ip[32];
            inet_ntop(AF_INET, &((struct sockaddr_in*)res->ai_addr)->sin_addr, ip, sizeof ip);
            printf("[iotest] example.com = %s (%ld ms)\n", ip, ms_since(&t0));

            /* 8. TCP bloqueante: HTTP GET */
            int s = socket(AF_INET, SOCK_STREAM, 0);
            r = connect(s, res->ai_addr, res->ai_addrlen);
            check("connect TCP porta 80", r == 0);
            const char* req = "GET / HTTP/1.0\r\nHost: example.com\r\nConnection: close\r\n\r\n";
            send(s, req, strlen(req), 0);
            int got = 0, n;
            while ((n = recv(s, buf + got, sizeof buf - 1 - got, 0)) > 0 && got < (int)sizeof buf - 1) got += n;
            buf[got] = 0;
            check("resposta HTTP", strncmp(buf, "HTTP/1.", 7) == 0);
            char* eol = strchr(buf, '\r');
            if (eol) *eol = 0;
            printf("[iotest] %s (%d bytes)\n", buf, got);
            close(s);

            /* 9. connect nao bloqueante + poll(POLLOUT) + SO_ERROR */
            s = socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK, 0);
            r = connect(s, res->ai_addr, res->ai_addrlen);
            check("connect nao bloqueante -> EINPROGRESS", r == -1 && errno == EINPROGRESS);
            struct pollfd sp = {.fd = s, .events = POLLOUT};
            r = poll(&sp, 1, 5000);
            int err = -1;
            socklen_t el2 = sizeof err;
            getsockopt(s, SOL_SOCKET, SO_ERROR, &err, &el2);
            check("poll(POLLOUT) e SO_ERROR = 0", r == 1 && (sp.revents & POLLOUT) && err == 0);
            close(s);
            freeaddrinfo(res);
        }
    }

    printf("[iotest] fim: %d falha(s)\n", fails);
    return fails;
}
