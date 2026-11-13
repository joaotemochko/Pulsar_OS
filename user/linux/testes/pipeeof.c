// Le ate EOF de um pipe cujo escritor (outro processo) fecha; repete muitas
// vezes. Um despertar perdido no fechamento trava o leitor (alarme detecta).
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <signal.h>
#include <sys/wait.h>
static volatile int it;
static void al(int s){(void)s; printf("PIPEEOF TRAVOU na iteracao %d\n",it); fflush(stdout); _exit(1);}
int main(int argc,char**argv){
  int n=argc>1?atoi(argv[1]):500, sz=argc>2?atoi(argv[2]):8192; signal(SIGALRM,al);
  char*buf=malloc(sz); memset(buf,'x',sz);
  for(it=0;it<n;it++){
    int p[2]; pipe(p); alarm(20);
    pid_t w=fork();
    if(w==0){ close(p[0]); for(int k=0;k<4;k++) write(p[1],buf,sz); close(p[1]); _exit(0);}
    close(p[1]); long tot=0; char r[4096]; ssize_t k; while((k=read(p[0],r,sizeof r))>0) tot+=k;
    close(p[0]); waitpid(w,0,0); alarm(0);
    if(tot!=4L*sz){printf("PIPEEOF bytes=%ld esperado=%ld it=%d\n",tot,4L*sz,it);}
  }
  printf("PIPEEOF ok n=%d sz=%d\n",n,sz); return 0;
}
