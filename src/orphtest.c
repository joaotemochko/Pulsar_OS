#include <stdio.h>
#include <stdlib.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <sys/statfs.h>
static unsigned long long ff(void){struct statfs s;statfs("/tmp",&s);return s.f_ffree;}
int main(){
  unsigned long long a=ff();
  for(int i=0;i<100;i++){ pid_t p=fork(); if(p==0){ char n[64]; snprintf(n,64,"/tmp/orf%d",i);
      int fd=open(n,O_CREAT|O_RDWR,0644); write(fd,"x",1); void*m=mmap(0,4096,PROT_READ,MAP_PRIVATE,fd,0); close(fd); unlink(n); (void)m; _exit(0);} waitpid(p,0,0); }
  unsigned long long b=ff();
  for(int i=0;i<100;i++){ pid_t p=fork(); if(p==0){ int fd=open("/tmp/orfb",O_CREAT|O_RDWR,0644); unlink("/tmp/orfb"); (void)fd; _exit(0);} waitpid(p,0,0); }
  unsigned long long c=ff();
  printf("ORPH mmap+unlink+exit vazou=%lld ; open+unlink+exit vazou=%lld\n",(long long)(a-b),(long long)(b-c));
  return 0;
}
