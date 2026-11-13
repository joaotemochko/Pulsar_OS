// executa os programas em sequencia (argv: lista separada por ';' em /etc/runlist)
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/wait.h>
#include <time.h>
int main(int argc,char**argv){
  FILE*f=fopen(argc>1?argv[1]:"/etc/runlist","r"); if(!f){printf("RUNALL sem lista\n");return 1;}
  char line[512];
  while(fgets(line,sizeof line,f)){
    line[strcspn(line,"\n")]=0; if(!line[0]||line[0]=='#') continue;
    char*av[32];int n=0;char*s=strtok(line," ");while(s&&n<31){av[n++]=s;s=strtok(0," ");}av[n]=0;
    struct timespec t0,t1;clock_gettime(CLOCK_MONOTONIC,&t0);
    printf("RUN-BEGIN %s\n",av[0]);fflush(stdout);
    pid_t p=fork();
    if(p==0){execv(av[0],av);printf("execv falhou\n");_exit(127);}
    int st=0;waitpid(p,&st,0);clock_gettime(CLOCK_MONOTONIC,&t1);
    printf("RUN-END %s status=%d sinal=%d tempo_ms=%.1f\n",av[0],WIFEXITED(st)?WEXITSTATUS(st):-1,WIFSIGNALED(st)?WTERMSIG(st):0,
      (t1.tv_sec-t0.tv_sec)*1e3+(t1.tv_nsec-t0.tv_nsec)/1e6);fflush(stdout);
  }
  printf("ALLDONE\n");fflush(stdout);
  return 0;
}
