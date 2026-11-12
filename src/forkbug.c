#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/wait.h>
int main(int argc,char**argv){
  if(argc>1) return 0;
  char *h=malloc(64); strcpy(h,"heap-ok");
  printf("antes: %p %s\n",(void*)h,h); fflush(stdout);
  const char* mode[]={"fork+_exit","fork+execve"};
  for(int m=0;m<2;m++){
    pid_t p=fork();
    if(p==0){ if(m==1){char*av[]={argv[0],"x",0};execv(argv[0],av);} _exit(0);}
    waitpid(p,0,0);
    printf("depois de %s: %s\n",mode[m],h); fflush(stdout);
  }
  printf("FORKBUG fim ok\n");
  return 0;
}
