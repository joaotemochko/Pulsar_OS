#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/stat.h>
#include <sys/statfs.h>
static void df(const char*t){struct statfs s;statfs("/tmp",&s);struct stat st;stat("/tmp",&st);printf("DL %s bfree=%llu ffree=%llu tmp_size=%lld\n",t,(unsigned long long)s.f_bfree,(unsigned long long)s.f_ffree,(long long)st.st_size);fflush(stdout);}
int main(int argc,char**argv){
  int mode=argc>1?atoi(argv[1]):0; df("inicio");
  for(int i=0;i<3000;i++){
    char d[64],f[96]; snprintf(d,64,"/tmp/LTP_%c%c%c%06d",'a'+i%26,'a'+(i/26)%26,'x',i);
    if(mkdir(d,0700)){printf("DL falha mkdir i=%d errno=%d %s\n",i,errno,strerror(errno));df("falha");return 1;}
    snprintf(f,96,"%s/arquivo",d); int fd=open(f,O_CREAT|O_RDWR,0644);
    if(fd<0){printf("DL falha open i=%d errno=%d %s\n",i,errno,strerror(errno));df("falha");return 1;}
    write(fd,"abc",3); close(fd); unlink(f); rmdir(d);
    if(i%500==0){char t[16];snprintf(t,16,"i=%d",i);df(t);}
  }
  df("fim"); return 0;
}
