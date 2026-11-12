// Executor do LTP: um teste por vez, com prazo, e resultado pelo codigo de saida.
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <signal.h>
#include <time.h>
#include <errno.h>
#include <grp.h>
#include <sys/wait.h>
#include <sys/mount.h>
#include <sys/stat.h>
static double ms(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec*1e3+t.tv_nsec/1e6;}
int main(int argc,char**argv){
  int lim = argc>2? atoi(argv[2]) : 120;      // prazo por teste (s)
  if(getuid()==0){ // so no Linux: ambiente minimo e depois usuario 1000, como no Pulsar
    mount("proc","/proc","proc",0,0); mount("sysfs","/sys","sysfs",0,0);
    mkdir("/dev/shm",01777); mount("tmpfs","/dev/shm","tmpfs",0,"mode=1777");
    setgroups(0,0); setresgid(1000,1000,1000); setresuid(1000,1000,1000);
  }
  setenv("TMPDIR","/tmp",1); setenv("PATH","/opt/ltp:/bin:/usr/bin",1);
  setenv("LTPROOT","/opt/ltp",1); setenv("LTP_TIMEOUT_MUL","4",1); setenv("HOME","/home/user",1);
  chdir("/tmp");
  FILE*f=fopen(argc>1?argv[1]:"/etc/ltplist","r"); if(!f){printf("LTPRUN sem lista\n");return 1;}
  char line[512];
  while(fgets(line,sizeof line,f)){
    line[strcspn(line,"\n")]=0; if(!line[0]) continue;
    char*av[32];int n=0;char*s=strtok(line," ");char*tag=s;s=strtok(0," ");
    while(s&&n<31){av[n++]=s;s=strtok(0," ");}av[n]=0; if(!n) continue;
    char path[256]; snprintf(path,sizeof path,"/opt/ltp/%s",av[0]);
    printf("LTP-BEGIN %s\n",tag);fflush(stdout);
    double t0=ms(); pid_t p=fork();
    if(p==0){ setpgid(0,0); execv(path,av); _exit(127);}
    int st=0,done=0,to=0;
    while(!done){
      pid_t r=waitpid(p,&st,WNOHANG);
      if(r==p){done=1;break;}
      if(r<0){done=1;st=-1;break;}
      if(ms()-t0>lim*1000.0){kill(-p,SIGKILL);kill(p,SIGKILL);waitpid(p,&st,0);to=1;done=1;break;}
      struct timespec z={0,20000000};nanosleep(&z,0);
    }
    const char*res;
    if(to) res="TIMEOUT";
    else if(WIFSIGNALED(st)) res="CRASH";
    else { int c=WEXITSTATUS(st);
      if(c==0) res="PASS"; else if(c==127) res="NOEXEC"; else if(c&2) res="BROK"; else if(c&1) res="FAIL"; else if(c==32) res="CONF"; else if(c&4) res="WARN"; else res="OTHER"; }
    printf("LTP-RESULT %s %s code=%d sig=%d ms=%.0f\n",tag,res,WIFEXITED(st)?WEXITSTATUS(st):-1,WIFSIGNALED(st)?WTERMSIG(st):0,ms()-t0);fflush(stdout);
  }
  printf("ALLDONE\n");fflush(stdout);
  return 0;
}
