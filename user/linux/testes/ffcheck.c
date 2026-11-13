// Revalidacao do Firefox no Pulsar (roda pelo /etc/autorun).
// 1) Firefox headless gera um screenshot de uma pagina local;
// 2) o resultado e o tempo vao para a serial.
// Uso: ffcheck [/opt/firefox/firefox] [prazo_s]
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <signal.h>
#include <time.h>
#include <sys/stat.h>
#include <sys/wait.h>
static double now(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec+t.tv_nsec/1e9;}
int main(int argc,char**argv){
  const char*ff=argc>1?argv[1]:"/opt/firefox/firefox"; int lim=argc>2?atoi(argv[2]):1800;
  const char*png="/tmp/ffcheck.png"; unlink(png);
  FILE*h=fopen("/tmp/ffcheck.html","w"); fprintf(h,"<html><body style='background:#2a6'><h1>Pulsar ffcheck</h1><script>document.body.append(String(6*7))</script></body></html>\n"); fclose(h);
  setenv("MOZ_HEADLESS","1",1); setenv("HOME","/home/user",1); setenv("TMPDIR","/tmp",1);
  printf("FFCHECK inicio %s\n",ff); fflush(stdout);
  double t0=now(); pid_t p=fork();
  if(p==0){ setpgid(0,0); char*av[]={(char*)ff,"--headless","--no-remote","--profile","/tmp/ffprof","--screenshot",(char*)png,"file:///tmp/ffcheck.html",0};
    mkdir("/tmp/ffprof",0700); execv(ff,av); perror("execv"); _exit(127);}
  int st=0; for(;;){ pid_t r=waitpid(p,&st,WNOHANG); if(r==p)break; if(now()-t0>lim){kill(-p,SIGKILL);kill(p,SIGKILL);waitpid(p,&st,0);printf("FFCHECK TIMEOUT %d s\n",lim);break;} sleep(1);}
  struct stat sb; int ok=stat(png,&sb)==0 && sb.st_size>1000;
  printf("FFCHECK %s status=%d sinal=%d png_bytes=%lld segundos=%.0f\n",ok?"OK":"FALHA",WIFEXITED(st)?WEXITSTATUS(st):-1,WIFSIGNALED(st)?WTERMSIG(st):0,ok?(long long)sb.st_size:0LL,now()-t0);
  fflush(stdout); return ok?0:1;
}
