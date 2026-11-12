// Microbenchmarks da ABI Linux (mesmo binario estatico em Pulsar e Linux).
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <time.h>
#include <pthread.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <sys/syscall.h>
#include <sys/socket.h>

static double now_ns(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec*1e9+t.tv_nsec;}
static int RUNS=7;
static int cmpd(const void*a,const void*b){double x=*(double*)a,y=*(double*)b;return x<y?-1:x>y;}
typedef void (*fn)(long);
static const char*ONLY=0;
static void report(const char*name,fn f,long iters){
  double v[16];
  if(ONLY&&!strstr(ONLY,name)) return;
  f(iters/10+1); // aquecimento
  for(int r=0;r<RUNS;r++){double t0=now_ns();f(iters);v[r]=(now_ns()-t0)/iters;}
  qsort(v,RUNS,sizeof(double),cmpd);
  printf("BENCH %-18s iters=%ld mediana_ns=%.1f min_ns=%.1f max_ns=%.1f\n",name,iters,v[RUNS/2],v[0],v[RUNS-1]);
  fflush(stdout);
}
static void b_getppid(long n){for(long i=0;i<n;i++) syscall(SYS_getppid);}
static void b_clock(long n){struct timespec t;for(long i=0;i<n;i++) syscall(SYS_clock_gettime,CLOCK_MONOTONIC,&t);}
static int zfd,wfd;
static void b_read0(long n){char c;for(long i=0;i<n;i++) read(zfd,&c,1);}
static void b_writenull(long n){char c=0;for(long i=0;i<n;i++) write(wfd,&c,1);}
static char statpath[64];
static void b_stat(long n){struct stat st;for(long i=0;i<n;i++) stat(statpath,&st);}
static void b_openclose(long n){for(long i=0;i<n;i++){int f=open(statpath,O_RDONLY);close(f);}}
static void b_pipe1(long n){int p[2];pipe(p);char c=0;for(long i=0;i<n;i++){write(p[1],&c,1);read(p[0],&c,1);}close(p[0]);close(p[1]);}
static void pingpong(long n,int unixsock){
  int a[2],b[2];
  if(unixsock){socketpair(AF_UNIX,SOCK_STREAM,0,a);b[0]=a[1];b[1]=a[0];}
  else{pipe(a);pipe(b);}
  // a: pai->filho ; b: filho->pai
  int toC=unixsock?a[0]:a[1], fromC=unixsock?a[0]:b[0];
  int cin=unixsock?a[1]:a[0], cout=unixsock?a[1]:b[1];
  pid_t p=fork(); char c=0;
  if(p==0){for(long i=0;i<n;i++){read(cin,&c,1);write(cout,&c,1);}_exit(0);}
  for(long i=0;i<n;i++){write(toC,&c,1);read(fromC,&c,1);}
  waitpid(p,0,0);
  close(a[0]);close(a[1]);if(!unixsock){close(b[0]);close(b[1]);}
}
static void b_pp_pipe(long n){pingpong(n,0);}
static void b_pp_unix(long n){pingpong(n,1);}
static void b_fork(long n){for(long i=0;i<n;i++){pid_t p=fork();if(p==0)_exit(0);waitpid(p,0,0);}}
static char *self;
static void b_forkexec(long n){for(long i=0;i<n;i++){pid_t p=fork();if(p==0){char*av[]={self,"nada",0};char*ev[]={0};execve(self,av,ev);_exit(127);}waitpid(p,0,0);}}
static void b_mmap(long n){for(long i=0;i<n;i++){char*m=mmap(0,4096,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANONYMOUS,-1,0);m[0]=1;munmap(m,4096);}}
static void b_fault(long n){size_t sz=n*4096;char*m=mmap(0,sz,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANONYMOUS,-1,0);for(long i=0;i<n;i++)m[i*4096]=1;munmap(m,sz);}
static void*tf(void*x){return x;}
static void b_thread(long n){for(long i=0;i<n;i++){pthread_t t;pthread_create(&t,0,tf,0);pthread_join(t,0);}}
static void b_mtx(long n){pthread_mutex_t m=PTHREAD_MUTEX_INITIALIZER;for(long i=0;i<n;i++){pthread_mutex_lock(&m);pthread_mutex_unlock(&m);}}
static char buf[1<<20];
static void b_fwrite(long n){ // escreve e le 1 MiB em arquivo
  for(long i=0;i<n;i++){int f=open(statpath,O_WRONLY|O_TRUNC);write(f,buf,sizeof buf);close(f);f=open(statpath,O_RDONLY);read(f,buf,sizeof buf);close(f);}
}
int main(int argc,char**argv){
  if(argc>1&&!strcmp(argv[1],"nada")) return 0;
  self=argv[0];
  const char*tmp=getenv("BENCH_DIR"); if(!tmp) tmp="/tmp";
  snprintf(statpath,sizeof statpath,"%s/lbench.dat",tmp);
  int f=open(statpath,O_CREAT|O_WRONLY|O_TRUNC,0644); write(f,"x",1); close(f);
  zfd=open("/dev/zero",O_RDONLY); wfd=open("/dev/null",O_WRONLY);
  long k = argc>1? atol(argv[1]) : 1; // fator de escala
  if(argc>2) ONLY=argv[2];
  printf("BEGIN lbench\n");
  report("getppid",b_getppid,20000*k);
  report("clock_gettime",b_clock,20000*k);
  report("read_dev_zero",b_read0,20000*k);
  report("write_dev_null",b_writenull,20000*k);
  report("stat",b_stat,5000*k);
  report("open_close",b_openclose,5000*k);
  report("pipe_rw_1B",b_pipe1,10000*k);
  report("pingpong_pipe",b_pp_pipe,2000*k);
  report("pingpong_unix",b_pp_unix,2000*k);
  report("mutex_uncontended",b_mtx,200000*k);
  report("mmap_touch_munmap",b_mmap,2000*k);
  report("page_fault",b_fault,4000*k);
  report("thread_create_join",b_thread,200*k);
  report("fork_exit_wait",b_fork,100*k);
  report("fork_exec_wait",b_forkexec,50*k);
  report("file_rw_1MiB",b_fwrite,10*k);
  unlink(statpath);
  printf("END lbench\n");
  return 0;
}
