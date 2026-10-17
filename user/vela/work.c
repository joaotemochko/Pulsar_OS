/* work.c — paralelismo do Vela.
 *
 * Threads (cada uma pode rodar num nucleo diferente):
 *   - principal: interface, DOM, JavaScript, estilo, layout e pintura;
 *   - rede (net.c): todas as requisicoes, em paralelo, sem parar;
 *   - workers: trabalho pesado sem estado compartilhado — analisar CSS,
 *     decodificar imagens, rasterizar SVG.
 * Uma Task roda em um worker (run) e depois volta para a thread principal
 * (done), que e a unica que mexe no DOM e no resto do estado da pagina. */
#include "vela.h"

#define MAXW 4

static Mutex wq_lock = MUTEX_INIT;
static Task *wq_head = NULL, *wq_tail = NULL;
static volatile u32 wq_seq = 0;                 /* futex: muda a cada tarefa nova */

static Mutex done_lock = MUTEX_INIT;
static Task *done_head = NULL, *done_tail = NULL;
static volatile u32 outstanding = 0;            /* tarefas/requisicoes ainda nao entregues */

static int nworkers = 0;

static void push(Task** h, Task** t, Task* x){
    x->next = NULL;
    if(*t) (*t)->next = x; else *h = x;
    *t = x;
}

void work_hold(void){ __atomic_add_fetch(&outstanding, 1, __ATOMIC_ACQ_REL); }
int  work_busy(void){ return __atomic_load_n(&outstanding, __ATOMIC_ACQUIRE) != 0; }

/* entrega para a thread principal (chamado de qualquer thread) */
void work_post_main(Task* t){
    mutex_lock(&done_lock);
    push(&done_head, &done_tail, t);
    mutex_unlock(&done_lock);
}

/* tarefa para um worker; sem workers, roda aqui mesmo */
void work_submit(Task* t){
    if(!nworkers){
        void (*done)(Task*) = t->done;
        if(t->run) t->run(t);
        if(done) work_post_main(t);
        return;
    }
    mutex_lock(&wq_lock);
    push(&wq_head, &wq_tail, t);
    __atomic_add_fetch(&wq_seq, 1, __ATOMIC_RELEASE);
    mutex_unlock(&wq_lock);
    sys_futex_wake(&wq_seq, 1);
}

/* tarefa urgente (o estilo esperando por ajuda): na frente da fila */
void work_submit_front(Task* t){
    if(!nworkers){ void (*done)(Task*) = t->done; if(t->run) t->run(t); if(done) work_post_main(t); return; }
    mutex_lock(&wq_lock);
    t->next = wq_head; wq_head = t; if(!wq_tail) wq_tail = t;
    __atomic_add_fetch(&wq_seq, 1, __ATOMIC_RELEASE);
    mutex_unlock(&wq_lock);
    sys_futex_wake(&wq_seq, 1);
}

static void worker(void* arg){
    (void)arg;
    for(;;){
        u32 seen = __atomic_load_n(&wq_seq, __ATOMIC_ACQUIRE);
        mutex_lock(&wq_lock);
        Task* t = wq_head;
        if(t){ wq_head = t->next; if(!wq_head) wq_tail = NULL; }
        mutex_unlock(&wq_lock);
        if(!t){ sys_futex_wait(&wq_seq, seen, 0); continue; }
        void (*done)(Task*) = t->done;     /* sem done: a tarefa e do chamador, nada volta */
        if(t->run) t->run(t);
        if(done) work_post_main(t);
    }
}

/* thread principal: roda o `done` das tarefas que terminaram */
int work_poll(void){
    mutex_lock(&done_lock);
    Task* t = done_head;
    done_head = done_tail = NULL;
    mutex_unlock(&done_lock);
    int n = 0;
    while(t){
        Task* nx = t->next;
        __atomic_sub_fetch(&outstanding, 1, __ATOMIC_ACQ_REL);
        if(t->done) t->done(t);
        t = nx; n++;
    }
    return n;
}

void work_init(void){
    u32 cpus = cpu_count();
    /* nucleos livres para workers (a principal e a de rede ficam com dois) */
    int w = (int)cpus - 2;
    if(w < 1) w = 1;
    if(w > MAXW) w = MAXW;
    for(int i = 0; i < w; i++) if(thread_spawn(worker, NULL, 512 * 1024) >= 0) nworkers++;
    net_init();
    printf("[vela] %u CPUs: 1 thread de rede + %d worker(s)\n", cpus, nworkers);
}
