#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <time.h>
#include <sys/wait.h>
static double ms(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec*1e3+t.tv_nsec/1e6;}
int main(int argc,char**argv){
  int n=argc>1?atoi(argv[1]):300; double t0=ms(),tl=t0;
  for(int i=1;i<=n;i++){
    pid_t p=fork(); if(p==0) _exit(0);
    if(p<0){printf("FL fork falhou em %d\n",i);fflush(stdout);return 1;}
    waitpid(p,0,0);
    if(i%10==0){double t=ms();printf("FL %d forks total_ms=%.0f ultimos10_ms=%.1f\n",i,t-t0,t-tl);fflush(stdout);tl=t;}
  }
  printf("FL fim\n");return 0;
}
