// N processos vivos ao mesmo tempo, cada um com um padrao proprio na memoria,
// verificado depois de muitas trocas de contexto. Com mais espacos que ASIDs,
// um reuso incorreto de ASID faz um processo ler a memoria de outro.
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <sched.h>
#include <time.h>
#include <sys/wait.h>
#define PAGES 64
static unsigned long buf[PAGES*512];
int main(int argc,char**argv){
  int n=argc>1?atoi(argv[1]):40, rounds=argc>2?atoi(argv[2]):200, bad=0;
  for(int i=0;i<n;i++){
    pid_t p=fork();
    if(p==0){ unsigned long tag=0xA5A5000000000000ul|((unsigned long)getpid()<<16);
      for(int r=0;r<rounds;r++){
        for(int k=0;k<PAGES*512;k+=512) buf[k]=tag|r;
        sched_yield(); struct timespec z={0,200000}; nanosleep(&z,0);
        for(int k=0;k<PAGES*512;k+=512) if(buf[k]!=(tag|r)) _exit(1);
      }
      _exit(0); }
  }
  for(int i=0;i<n;i++){ int st; wait(&st); if(!WIFEXITED(st)||WEXITSTATUS(st)) bad++; }
  printf("ASIDTEST processos=%d rodadas=%d corrompidos=%d\n",n,rounds,bad);
  return 0;
}
