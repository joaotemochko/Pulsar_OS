// Estresse SMP: threads e processos concorrentes exercitando VM, fork/exec,
// pipes, arquivos e sinais ao mesmo tempo, com verificacao de cada resultado.
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <signal.h>
#include <pthread.h>
#include <time.h>
#include <errno.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <stdatomic.h>
static atomic_long ops[6], errs[6];
static volatile int stop;
static double now(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec+t.tv_nsec/1e9;}
static void *w_mem(void*a){ long id=(long)a; while(!stop){ size_t n=16*4096; unsigned char*m=mmap(0,n,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANONYMOUS,-1,0);
  if(m==MAP_FAILED){errs[0]++;continue;} for(size_t i=0;i<n;i+=4096)m[i]=(unsigned char)(id+i/4096);
  for(size_t i=0;i<n;i+=4096) if(m[i]!=(unsigned char)(id+i/4096)) errs[0]++; munmap(m,n); ops[0]++; } return 0; }
static void *w_fork(void*a){ (void)a; while(!stop){ pid_t p=fork(); if(p==0){ char*v[]={"/bin/smpstress","filho",0}; execv(v[0],v); _exit(99);} 
  if(p<0){errs[1]++;continue;} int st; pid_t r=waitpid(p,&st,0); if(r!=p||!WIFEXITED(st)||WEXITSTATUS(st)!=7){ errs[1]++; printf("SMPERR fork r=%d errno=%d exited=%d code=%d sig=%d\n",(int)r,errno,WIFEXITED(st),WEXITSTATUS(st),WIFSIGNALED(st)?WTERMSIG(st):0); fflush(stdout);} ops[1]++; } return 0; }
static void *w_pipe(void*a){ (void)a; while(!stop){ int x[2],y[2]; if(pipe(x)||pipe(y)){errs[2]++;continue;} pid_t p=fork();
  if(p==0){ char c; for(int i=0;i<50;i++){ if(read(x[0],&c,1)!=1)_exit(1); c++; write(y[1],&c,1);} _exit(0);}
  char c=0; for(int i=0;i<50;i++){ char d=c; write(x[1],&c,1); if(read(y[0],&c,1)!=1||c!=d+1){errs[2]++;break;} }
  int st; waitpid(p,&st,0); if(!WIFEXITED(st)||WEXITSTATUS(st)) errs[2]++; close(x[0]);close(x[1]);close(y[0]);close(y[1]); ops[2]++; } return 0; }
static void *w_file(void*a){ long id=(long)a; char f[64],b[4096],r[4096]; while(!stop){ snprintf(f,64,"/tmp/smp_%ld",id); memset(b,(int)(id+ops[3]),sizeof b);
  int fd=open(f,O_CREAT|O_RDWR|O_TRUNC,0644); if(fd<0){errs[3]++;continue;} write(fd,b,sizeof b); lseek(fd,0,SEEK_SET);
  if(read(fd,r,sizeof r)!=sizeof r||memcmp(b,r,sizeof b)) errs[3]++; close(fd); unlink(f); ops[3]++; } return 0; }
static atomic_long got;
static void h(int s){(void)s; got++;}
static void *w_sig(void*a){ (void)a; while(!stop){ long g=got; kill(getpid(),SIGUSR1); struct timespec z={0,100000}; for(int i=0;i<100&&got==g;i++) nanosleep(&z,0); if(got==g) errs[4]++; ops[4]++; } return 0; }
static void *noop(void*a){return a;}
static void *w_threads(void*a){ (void)a; while(!stop){ pthread_t t; void*r; if(pthread_create(&t,0,noop,(void*)0x55)||pthread_join(t,&r)||r!=(void*)0x55) errs[5]++; ops[5]++; } return 0; }
int main(int argc,char**argv){
  if(argc>1&&!strcmp(argv[1],"filho")) return 7;
  int secs=argc>1?atoi(argv[1]):20; struct sigaction sa; memset(&sa,0,sizeof sa); sa.sa_handler=h; sa.sa_flags=SA_RESTART; sigaction(SIGUSR1,&sa,0);
  void*(*fn[])(void*)={w_mem,w_mem,w_fork,w_pipe,w_file,w_file,w_sig,w_threads};
  pthread_t t[8]; double t0=now();
  for(long i=0;i<8;i++) pthread_create(&t[i],0,fn[i],(void*)i);
  while(now()-t0<secs){ struct timespec z={0,200000000}; nanosleep(&z,0);} stop=1;
  for(int i=0;i<8;i++) pthread_join(t[i],0);
  const char*n[]={"mmap","fork+exec","pipe","file","signal","thread"}; long te=0;
  printf("SMPSTRESS segundos=%.1f",now()-t0); for(int i=0;i<6;i++){printf(" %s=%ld/%ld",n[i],(long)ops[i],(long)errs[i]); te+=errs[i];} printf(" erros=%ld\n",te);
  return te?1:0;
}
