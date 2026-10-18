/* sigtest — sinais POSIX na personalidade Linux do Pulsar: tratadores,
 * SA_SIGINFO, mascaras, siglongjmp para sair de um SIGSEGV, tratador que
 * corrige a falha (mprotect) e retorna, ucontext alterado pelo tratador,
 * sigaltstack, setitimer/pause, SIGCHLD, EINTR, pthread_kill, SIGPIPE e
 * morte por sinal vista no waitpid. */
#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <setjmp.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/time.h>
#include <sys/wait.h>
#include <time.h>
#include <ucontext.h>
#include <unistd.h>

static int fails;
static void check(const char* what, int ok) {
    printf("[sigtest] %s %s\n", ok ? "ok   " : "FALHA", what);
    if (!ok) fails++;
}

static volatile sig_atomic_t got, got_info_ok, chld, alrm;
static void on_usr1(int s) { got = s; }
static void on_info(int s, siginfo_t* si, void* uc) {
    (void)uc;
    got_info_ok = s == SIGUSR2 && si->si_signo == SIGUSR2 && si->si_pid == getpid();
}

static sigjmp_buf jb;
static void on_segv_jump(int s) { (void)s; siglongjmp(jb, 1); }

static char* page;
static volatile int fixes;
static void on_segv_fix(int s, siginfo_t* si, void* uc) {
    (void)s; (void)uc;
    /* como um motor JS: o endereco da falha diz qual pagina liberar */
    if ((char*)si->si_addr >= page && (char*)si->si_addr < page + 4096) {
        mprotect(page, 4096, PROT_READ | PROT_WRITE);
        fixes++;
    } else {
        _exit(90);
    }
}

static volatile int skipped;
static void on_segv_skip(int s, siginfo_t* si, void* ucv) {
    (void)s; (void)si;
    ucontext_t* uc = ucv;
#if defined(__aarch64__)
    uc->uc_mcontext.pc += 4;                  /* pula a instrucao que falhou */
#else
    uc->uc_mcontext.__gregs[0] += 4;
#endif
    skipped++;
}

static char altstack[64 * 1024];
static volatile int on_alt;
static void on_usr1_alt(int s) {
    (void)s;
    char local;
    on_alt = &local >= altstack && &local < altstack + sizeof altstack;
}

static void on_chld(int s) { (void)s; chld++; }
static void on_alrm(int s) { (void)s; alrm++; }

static volatile double fp_scratch;
static void on_fp(int s) {
    /* suja os registradores de FP dentro do tratador */
    double a = s;
    for (int i = 0; i < 100; i++) a = a * 1.5 + 0.25;
    fp_scratch = a;
}

static __thread int thread_hits;
static volatile int thread_flag;
static void on_thread_sig(int s) { (void)s; thread_hits++; thread_flag = 1; }
static void* sleeper(void* arg) {
    (void)arg;
    while (!thread_flag) usleep(1000);
    return (void*)(long)thread_hits;
}

int main(void) {
    struct sigaction sa;

    /* 1. tratador simples */
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = on_usr1;
    sigaction(SIGUSR1, &sa, NULL);
    raise(SIGUSR1);
    check("raise(SIGUSR1) chama o tratador", got == SIGUSR1);

    /* 2. SA_SIGINFO com si_pid */
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = on_info;
    sa.sa_flags = SA_SIGINFO;
    sigaction(SIGUSR2, &sa, NULL);
    kill(getpid(), SIGUSR2);
    check("SA_SIGINFO: si_signo e si_pid", got_info_ok);

    /* 3. mascara: bloqueado fica pendente, desbloquear entrega */
    sigset_t set, pend;
    sigemptyset(&set);
    sigaddset(&set, SIGUSR1);
    got = 0;
    sigprocmask(SIG_BLOCK, &set, NULL);
    raise(SIGUSR1);
    sigpending(&pend);
    check("bloqueado: nao entrega, fica pendente", got == 0 && sigismember(&pend, SIGUSR1));
    sigprocmask(SIG_UNBLOCK, &set, NULL);
    check("desbloquear entrega", got == SIGUSR1);

    /* 4. SIGSEGV + siglongjmp */
    signal(SIGSEGV, on_segv_jump);
    int jumped = 0;
    if (sigsetjmp(jb, 1) == 0) {
        *(volatile int*)8 = 1;
    } else {
        jumped = 1;
    }
    check("SIGSEGV + siglongjmp", jumped);

    /* 5. tratador corrige a falha e retorna (a instrucao roda de novo) */
    page = mmap(NULL, 4096, PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = on_segv_fix;
    sa.sa_flags = SA_SIGINFO;
    sigaction(SIGSEGV, &sa, NULL);
    page[100] = 42;
    check("tratador faz mprotect e a escrita completa", fixes == 1 && page[100] == 42);

    /* 6. tratador altera o PC no ucontext */
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = on_segv_skip;
    sa.sa_flags = SA_SIGINFO;
    sigaction(SIGSEGV, &sa, NULL);
    volatile long marker = 1;
#if defined(__aarch64__)
    __asm__ volatile("mov x9, #16\n str xzr, [x9]" ::: "x9", "memory");
#else
    __asm__ volatile(".option push\n.option norvc\nli t0, 16\nsd zero, 0(t0)\n.option pop" ::: "t0", "memory");
#endif
    marker = 2;
    check("uc_mcontext.pc alterado pula a instrucao", skipped == 1 && marker == 2);
    signal(SIGSEGV, SIG_DFL);

    /* 7. sigaltstack */
    stack_t ss = {.ss_sp = altstack, .ss_size = sizeof altstack, .ss_flags = 0};
    sigaltstack(&ss, NULL);
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = on_usr1_alt;
    sa.sa_flags = SA_ONSTACK;
    sigaction(SIGUSR1, &sa, NULL);
    raise(SIGUSR1);
    check("SA_ONSTACK roda na pilha alternativa", on_alt);

    /* 8. setitimer + pause */
    signal(SIGALRM, on_alrm);
    struct itimerval it = {.it_value = {0, 100000}};
    setitimer(ITIMER_REAL, &it, NULL);
    int r = pause();
    check("setitimer(100 ms) + pause -> SIGALRM, EINTR", alrm == 1 && r == -1 && errno == EINTR);

    /* 9. SIGCHLD e nanosleep interrompido */
    signal(SIGCHLD, on_chld);
    signal(SIGUSR1, on_usr1);
    got = 0;
    pid_t pid = fork();
    if (pid == 0) {
        usleep(50000);
        kill(getppid(), SIGUSR1);
        _exit(0);
    }
    struct timespec ts = {5, 0};
    r = nanosleep(&ts, NULL);
    check("nanosleep interrompido por sinal -> EINTR", r == -1 && errno == EINTR && got == SIGUSR1);
    waitpid(pid, NULL, 0);
    check("SIGCHLD entregue ao pai", chld >= 1);

    /* 10. registradores de FP preservados */
    signal(SIGUSR1, on_fp);
    double x = 1.0 / 3.0, y = x;
    raise(SIGUSR1);
    for (int i = 0; i < 10; i++) y = y * 3.0 - 0.0;
    check("FP intacto depois do tratador", x == 1.0 / 3.0 && y > 19682.0 && y < 19684.0);

    /* 11. pthread_kill: o tratador roda na thread certa */
    signal(SIGUSR2, on_thread_sig);
    pthread_t th;
    pthread_create(&th, NULL, sleeper, NULL);
    usleep(20000);
    pthread_kill(th, SIGUSR2);
    void* hits;
    pthread_join(th, &hits);
    check("pthread_kill entrega na thread alvo", (long)hits == 1 && thread_hits == 0);

    /* 12. morte por sinal vista pelo pai */
    pid = fork();
    if (pid == 0) { raise(SIGTERM); _exit(0); }
    int st;
    waitpid(pid, &st, 0);
    check("filho morto por SIGTERM: WIFSIGNALED", WIFSIGNALED(st) && WTERMSIG(st) == SIGTERM);

    /* 13. SIGPIPE ignorado -> EPIPE */
    signal(SIGPIPE, SIG_IGN);
    int p[2];
    pipe(p);
    close(p[0]);
    r = write(p[1], "x", 1);
    check("SIGPIPE ignorado: write -> EPIPE", r == -1 && errno == EPIPE);
    close(p[1]);

    /* 14. SIGPIPE padrao mata o processo */
    pid = fork();
    if (pid == 0) {
        signal(SIGPIPE, SIG_DFL);
        pipe(p);
        close(p[0]);
        write(p[1], "x", 1);
        _exit(0);
    }
    waitpid(pid, &st, 0);
    check("SIGPIPE padrao encerra", WIFSIGNALED(st) && WTERMSIG(st) == SIGPIPE);

    printf("[sigtest] fim: %d falha(s)\n", fails);
    return fails;
}
