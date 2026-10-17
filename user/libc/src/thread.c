/* thread.c — threads do Pulsar e mutex sobre futex.
 *
 * As threads compartilham o espaco de enderecamento e o heap do processo;
 * o kernel as escalona em qualquer CPU. O mutex segue o desenho classico
 * de futex (Drepper): CAS 0->1 no caminho rapido, um pouco de espera ativa
 * e depois FUTEX_WAIT com o valor 2 ("ha gente esperando"). */
#include <stdlib.h>
#include "pulsar.h"

int threads_started = 0;

void mutex_lock(Mutex* m){
    u32 c = 0;
    if(__atomic_compare_exchange_n(&m->v, &c, 1, 0, __ATOMIC_ACQUIRE, __ATOMIC_RELAXED)) return;
    for(int i = 0; i < 64; i++){
        c = 0;
        if(__atomic_compare_exchange_n(&m->v, &c, 1, 0, __ATOMIC_ACQUIRE, __ATOMIC_RELAXED)) return;
    }
    if(c != 2) c = __atomic_exchange_n(&m->v, 2, __ATOMIC_ACQUIRE);
    while(c != 0){
        sys_futex_wait(&m->v, 2, 0);
        c = __atomic_exchange_n(&m->v, 2, __ATOMIC_ACQUIRE);
    }
}

void mutex_unlock(Mutex* m){
    if(__atomic_exchange_n(&m->v, 0, __ATOMIC_RELEASE) == 2) sys_futex_wake(&m->v, 1);
}

typedef struct { void (*fn)(void*); void* arg; } Boot;

/* o kernel entra aqui com o Boot no primeiro registrador; ao terminar, so
 * esta thread sai */
static void thread_entry(Boot* b){
    void (*fn)(void*) = b->fn; void* arg = b->arg;
    free(b);
    fn(arg);
    sys_exit();
}

i64 thread_spawn(void (*fn)(void*), void* arg, u64 stack){
    threads_started = 1;                  /* malloc passa a travar */
    Boot* b = (Boot*)malloc(sizeof *b);
    u8* st = (u8*)malloc(stack);
    if(!b || !st){ free(b); free(st); return -1; }
    b->fn = fn; b->arg = arg;
    u64 top = ((u64)(st + stack)) & ~(u64)15;
    i64 t = sys_thread_create((u64)thread_entry, top, (u64)b);
    if(t < 0){ free(b); free(st); }
    return t;
}

u32 cpu_count(void){
    SysInfo si; si.ncpus = 1;
    sys_sysinfo(&si);
    return si.ncpus ? si.ncpus : 1;
}
