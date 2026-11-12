/* dyntest — programa ligado dinamicamente (PIE + /lib/ld-musl-<arch>.so.1):
 * usa libc.so e libdemo.so carregadas pelo ligador da musl, e tambem
 * dlopen/dlsym em tempo de execucao. */
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "demo.h"

static int fails;
static void check(const char* what, int ok) {
    printf("[dyntest] %s %s\n", ok ? "ok   " : "FALHA", what);
    if (!ok) fails++;
}

int main(void) {
    check("funcao de libdemo.so", demo_add(2, 3) == 5);
    check("construtor de libdemo.so rodou (dado global = 42)", demo_counter == 42);
    check("printf/strlen da libc.so", strlen(demo_name()) == 10);
    char* m = malloc(1 << 20);
    check("malloc da libc.so", m != NULL);
    free(m);
    void* h = dlopen("libdemo.so", RTLD_NOW);
    check("dlopen(libdemo.so)", h != NULL);
    int (*add)(int, int) = h ? (int (*)(int, int))dlsym(h, "demo_add") : NULL;
    check("dlsym(demo_add) = mesma funcao", add == demo_add && add(20, 22) == 42);
    check("dlsym inexistente -> NULL", h && dlsym(h, "nao_existe") == NULL);
    printf("[dyntest] main em %p, demo_add em %p\n", (void*)main, (void*)demo_add);
    printf("[dyntest] fim: %d falha(s)\n", fails);
    return fails;
}
