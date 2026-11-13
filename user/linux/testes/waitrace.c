// Reproduz a anomalia: fork+execve de um filho que sai com 7, com outras
// threads do pai fazendo fork/_exit(0), mmap e sinais ao mesmo tempo.
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <signal.h>
#include <pthread.h>
#include <errno.h>
#include <sys/wait.h>
#include <sys/mman.h>
static volatile int stop; static int mode;
static void h(int s){(void)s;}
static void *other_fork(void*a){(void)a; while(!stop){ pid_t p=fork(); if(p==0)_exit(0); int st; waitpid(p,&st,0);} return 0;}
static void *other_mem(void*a){(void)a; while(!stop){ char*m=mmap(0,1<<16,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANONYMOUS,-1,0); m[0]=1; munmap(m,1<<16);} return 0;}
static void *other_sig(void*a){(void)a; while(!stop){ kill(getpid(),SIGUSR1); usleep(200);} return 0;}
int main(int argc,char**argv){
  if(argc>1&&!strcmp(argv[1],"filho")) return 7;
  int n=argc>1?atoi(argv[1]):300; mode=argc>2?atoi(argv[2]):7;
  struct sigaction sa; memset(&sa,0,sizeof sa); sa.sa_handler=h; sa.sa_flags=SA_RESTART; sigaction(SIGUSR1,&sa,0);
  pthread_t t[3]; int nt=0;
  if(mode&1) pthread_create(&t[nt++],0,other_fork,0);
  if(mode&2) pthread_create(&t[nt++],0,other_mem,0);
  if(mode&4) pthread_create(&t[nt++],0,other_sig,0);
  int bad=0;
  for(int i=0;i<n;i++){
    pid_t p=fork(); if(p==0){ char*v[]={"/bin/waitrace","filho",0}; execv(v[0],v); _exit(99);}
    int st=0x5A5A; pid_t r; do r=waitpid(p,&st,0); while(r<0&&errno==EINTR);
    if(r!=p||!WIFEXITED(st)||WEXITSTATUS(st)!=7){ bad++; printf("WR i=%d p=%d r=%d errno=%d st=%#x\n",i,p,r,errno,st); fflush(stdout);}
  }
  stop=1; for(int i=0;i<nt;i++) pthread_join(t[i],0);
  printf("WAITRACE modo=%d n=%d anomalias=%d\n",mode,n,bad); return 0;
}
