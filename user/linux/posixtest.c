/* posixtest — teste da personalidade Linux do Pulsar OS.
 *
 * Programa C comum, ligado estaticamente a musl sem modificacoes: usa so a
 * API padrao (stdio, dirent, pthreads, malloc, time...). Roda pelo kernel
 * com as syscalls do Linux (kernel/src/linux.rs). Imprime "ok"/"FALHA" por
 * verificacao e termina com o numero de falhas como codigo de saida. */
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <math.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/utsname.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static int fails;

static void check(const char* what, int ok) {
    printf("[posixtest] %s %s\n", ok ? "ok   " : "FALHA", what);
    if (!ok) fails++;
}

static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static long counter;

static void* worker(void* arg) {
    long id = (long)arg;
    double acc = 0;
    for (int i = 0; i < 100000; i++) {
        pthread_mutex_lock(&lock);
        counter++;
        pthread_mutex_unlock(&lock);
        acc += sqrt((double)i) * id;
    }
    return (void*)(long)(acc > 0);
}

static __thread int tls_value = 7;
static void* tls_worker(void* arg) {
    tls_value = (int)(long)arg;
    usleep(20000);
    return (void*)(long)tls_value;
}

int main(int argc, char** argv, char** envp) {
    printf("[posixtest] argc=%d argv[0]=%s\n", argc, argv[0]);
    check("argv[1] == \"primeiro\"", argc > 1 && strcmp(argv[1], "primeiro") == 0);
    check("getenv(HOME)", getenv("HOME") && strcmp(getenv("HOME"), "/home/user") == 0);
    (void)envp;

    struct utsname u;
    check("uname", uname(&u) == 0 && strcmp(u.sysname, "Linux") == 0);
    printf("[posixtest] uname: %s %s %s\n", u.sysname, u.release, u.machine);

    /* malloc: pequeno (brk) e grande (mmap) */
    char* small = malloc(100);
    char* big = malloc(64 << 20);
    check("malloc pequeno e grande", small && big);
    memset(big, 0xAB, 64 << 20);
    check("memoria grande utilizavel", big[0] == (char)0xAB && big[(64 << 20) - 1] == (char)0xAB);
    free(big);
    char* r = realloc(small, 1 << 20);
    check("realloc", r != NULL);
    free(r);

    /* ponto flutuante e printf de double */
    char fb[64];
    snprintf(fb, sizeof fb, "%.6f %.3e", M_PI, 6.02214076e23);
    check("printf de double", strcmp(fb, "3.141593 6.022e+23") == 0);
    check("libm", fabs(sin(M_PI / 6) - 0.5) < 1e-12 && fabs(pow(2, 0.5) - M_SQRT2) < 1e-12);

    /* arquivos com stdio */
    check("chdir /tmp", chdir("/tmp") == 0);
    char cwd[256];
    check("getcwd == /tmp", getcwd(cwd, sizeof cwd) && strcmp(cwd, "/tmp") == 0);
    FILE* f = fopen("teste.txt", "w");                  /* relativo ao cwd */
    check("fopen relativo", f != NULL);
    for (int i = 0; i < 1000; i++) fprintf(f, "linha %d\n", i);
    fclose(f);
    struct stat st;
    check("stat /tmp/teste.txt", stat("/tmp/teste.txt", &st) == 0 && S_ISREG(st.st_mode) && st.st_size > 7000);
    f = fopen("/tmp/teste.txt", "r");
    char line[64];
    int n = 0, last = -1;
    while (fgets(line, sizeof line, f)) { sscanf(line, "linha %d", &last); n++; }
    fclose(f);
    check("fgets le 1000 linhas", n == 1000 && last == 999);

    check("mkdir", mkdir("/tmp/dirteste", 0755) == 0);
    check("mkdir repetido -> EEXIST", mkdir("/tmp/dirteste", 0755) == -1 && errno == EEXIST);
    check("rename", rename("/tmp/teste.txt", "/tmp/dirteste/movido.txt") == 0);
    check("open inexistente -> ENOENT", open("/nao/existe", O_RDONLY) == -1 && errno == ENOENT);
    check("strerror", strcmp(strerror(ENOENT), "No such file or directory") == 0);

    DIR* d = opendir("/tmp/dirteste");
    int seen = 0;
    struct dirent* e;
    while (d && (e = readdir(d))) if (strcmp(e->d_name, "movido.txt") == 0 && e->d_type == DT_REG) seen = 1;
    if (d) closedir(d);
    check("opendir/readdir", seen);

    d = opendir("/bin");
    int nbin = 0;
    while (d && (e = readdir(d))) nbin++;
    if (d) closedir(d);
    check("readdir /bin", nbin >= 3);

    check("symlink + readlink", symlink("dirteste/movido.txt", "/tmp/atalho") == 0);
    char lk[64] = {0};
    check("readlink", readlink("/tmp/atalho", lk, sizeof lk - 1) > 0 && strcmp(lk, "dirteste/movido.txt") == 0);
    check("access via link", access("/tmp/atalho", F_OK) == 0);
    check("unlink", unlink("/tmp/atalho") == 0 && unlink("/tmp/dirteste/movido.txt") == 0);
    check("rmdir", rmdir("/tmp/dirteste") == 0);

    /* mmap de arquivo */
    int fd = open("/welcome.txt", O_RDONLY);
    check("open /welcome.txt", fd >= 0);
    fstat(fd, &st);
    char* m = mmap(NULL, st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
    char head[8] = {0};
    if (m != MAP_FAILED) memcpy(head, m, 7);
    check("mmap de arquivo", m != MAP_FAILED && pread(fd, line, 7, 0) == 7 && memcmp(head, line, 7) == 0);
    close(fd);

    /* tempo */
    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    struct timespec nap = {0, 50 * 1000 * 1000};
    nanosleep(&nap, NULL);
    clock_gettime(CLOCK_MONOTONIC, &t1);
    long ms = (t1.tv_sec - t0.tv_sec) * 1000 + (t1.tv_nsec - t0.tv_nsec) / 1000000;
    check("nanosleep 50 ms", ms >= 49 && ms < 500);
    time_t now = time(NULL);
    check("time() plausivel (> 2024)", now > 1700000000);

    /* threads */
    pthread_t th[6];
    for (long i = 0; i < 6; i++) pthread_create(&th[i], NULL, worker, (void*)(i + 1));
    int all = 1;
    for (int i = 0; i < 6; i++) { void* rv; pthread_join(th[i], &rv); all &= rv == (void*)1; }
    check("6 threads com mutex", all && counter == 600000);

    pthread_t ta, tb;
    pthread_create(&ta, NULL, tls_worker, (void*)11);
    pthread_create(&tb, NULL, tls_worker, (void*)22);
    void *ra, *rb;
    pthread_join(ta, &ra);
    pthread_join(tb, &rb);
    check("TLS por thread", ra == (void*)11 && rb == (void*)22 && tls_value == 7);

    check("getpid/gettid", getpid() > 0 && getppid() >= 0);

    /* waitid: WNOWAIT so espia (o Firefox vigia os processos filhos assim);
       depois o waitpid ainda colhe o mesmo filho */
    pid_t wc = fork();
    if (wc == 0) _exit(7);
    siginfo_t si = {0};
    int wr = waitid(P_PID, wc, &si, WEXITED | WNOWAIT);
    check("waitid WNOWAIT ve o filho", wr == 0 && si.si_pid == wc && si.si_code == CLD_EXITED && si.si_status == 7);
    int wst = 0;
    check("waitpid colhe depois do WNOWAIT", waitpid(wc, &wst, 0) == wc && WIFEXITED(wst) && WEXITSTATUS(wst) == 7);
    check("getuid = usuario (1000)", getuid() == 1000 && geteuid() == 1000);
    printf("[posixtest] fim: %d falha(s)\n", fails);
    return fails;
}
