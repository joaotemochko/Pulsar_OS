#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <time.h>
#include <sys/wait.h>
static double ns(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec*1e9+t.tv_nsec;}
static int cmpd(const void*a,const void*b){double x=*(double*)a,y=*(double*)b;return x<y?-1:x>y;}
int main(int argc,char**argv){
  const char*t[]={"/bin/lbench","/bin/big4","/bin/big16","/bin/big64"};
  long it[]={30,30,15,6};
  printf("BEGIN execsz\n");
  for(int k=0;k<4;k++){
    double v[7];
    for(int r=0;r<7;r++){ double t0=ns();
      for(long i=0;i<it[k];i++){pid_t p=fork(); if(p==0){char*av[]={(char*)t[k],"nada",0};char*ev[]={0};execve(t[k],av,ev);_exit(127);} int st; waitpid(p,&st,0); if(!WIFEXITED(st)||WEXITSTATUS(st)){printf("falha %s\n",t[k]);}}
      v[r]=(ns()-t0)/it[k]; }
    qsort(v,7,sizeof(double),cmpd);
    printf("BENCH exec_%s iters=%ld mediana_ns=%.1f min_ns=%.1f max_ns=%.1f\n",t[k]+5,it[k],v[3],v[0],v[6]); fflush(stdout);
  }
  printf("END execsz\n"); return 0;
}
