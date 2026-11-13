#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/wait.h>
int main(int argc,char**argv){
  if(argc>1) return 0;
  char *h=malloc(64); strcpy(h,"heap-ok");
  FILE*f=fopen("/etc/runlist","r"); char l[256];
  printf("FB2 h=%p f=%p\n",(void*)h,(void*)f); fflush(stdout);
  fgets(l,sizeof l,f); printf("FB2 linha1 ok\n"); fflush(stdout);
  pid_t p=fork();
  if(p==0){char*av[]={"/bin/forkbug","x",0};execv(av[0],av);_exit(127);}
  waitpid(p,0,0);
  printf("FB2 h depois: %s\n",h); fflush(stdout);
  printf("FB2 f->? lendo\n"); fflush(stdout);
  fgets(l,sizeof l,f); printf("FB2 linha2 ok\n"); fflush(stdout);
  return 0;
}
