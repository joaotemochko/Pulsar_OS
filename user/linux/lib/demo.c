/* libdemo.so — biblioteca compartilhada de exemplo/teste para o ligador
 * dinamico (ld-musl) no Pulsar: funcao, dado global e construtor. */
#include "demo.h"

int demo_counter = 40;
static int constructed;

__attribute__((constructor)) static void demo_init(void) { constructed = 1; demo_counter += 2; }

int demo_add(int a, int b) { return a + b + (constructed ? 0 : 1000); }
const char* demo_name(void) { return "libdemo.so"; }
