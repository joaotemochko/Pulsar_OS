// Verifica a semantica das chamadas que antes eram stubs de sucesso.
// Referencia: Linux sem CONFIG_SECCOMP e sem CONFIG_NAMESPACES, processo
// sem privilegios (uid 1000), que e exatamente o ambiente do Pulsar.
#define _GNU_SOURCE
#include <stdio.h>
#include <string.h>
#include <errno.h>
#include <unistd.h>
#include <fcntl.h>
#include <sched.h>
#include <sys/syscall.h>
#include <sys/prctl.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <sys/file.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <sys/resource.h>
#include <linux/capability.h>
static int fails=0;
static void chk(const char*cat,const char*name,long r,int exp_err){
  int e = r<0? errno:0;
  int ok = exp_err? (r<0 && e==exp_err) : (r>=0);
  printf("STUB %s %-34s ret=%ld errno=%-3d esperado=%s %s\n",cat,name,r,e,exp_err?strerror(exp_err):"sucesso",ok?"OK":"DIFERE");
  if(!ok) fails++;
}
static void sem(const char*name,int linux_like){ printf("STUB semantica %-34s %s\n",name,linux_like?"OK":"DIFERE"); if(!linux_like) fails++; }
int main(void){
  if(getuid()==0){ setgroups(0,0); setresgid(1000,1000,1000); setresuid(1000,1000,1000); }
  printf("BEGIN stubtest uid=%d\n",getuid());
  char d[]="/tmp/stubtest"; mkdir(d,0755);
  // seguranca
  chk("seg","seccomp(STRICT)",syscall(SYS_seccomp,0,0,0),ENOSYS);
  chk("seg","prctl(PR_SET_SECCOMP)",prctl(PR_SET_SECCOMP,2,0,0,0),EINVAL);
  chk("seg","prctl(PR_GET_SECCOMP)",prctl(PR_GET_SECCOMP,0,0,0,0),EINVAL);
  chk("seg","setuid(0)",syscall(SYS_setuid,0),EPERM);
  chk("seg","setgid(0)",syscall(SYS_setgid,0),EPERM);
  chk("seg","setresuid(0,0,0)",syscall(SYS_setresuid,0,0,0),EPERM);
  chk("seg","setreuid(-1,0)",syscall(SYS_setreuid,-1,0),EPERM);
  chk("seg","setuid(proprio uid)",syscall(SYS_setuid,getuid()),0);
  chk("seg","setresuid(-1,-1,-1)",syscall(SYS_setresuid,-1,-1,-1),0);
  struct __user_cap_header_struct h={_LINUX_CAPABILITY_VERSION_3,0}; struct __user_cap_data_struct c[2];
  memset(c,0,sizeof c); long r=syscall(SYS_capget,&h,c);
  sem("capget sem capacidades",r==0 && c[0].effective==0 && c[0].permitted==0);
  struct __user_cap_data_struct up[2]; memset(up,0,sizeof up); up[0].effective=up[0].permitted=1u<<CAP_NET_ADMIN;
  chk("seg","capset(+CAP_NET_ADMIN)",syscall(SYS_capset,&h,up),EPERM);
  memset(up,0,sizeof up);
  chk("seg","capset(vazio)",syscall(SYS_capset,&h,up),0);
  chk("seg","unshare(CLONE_NEWUSER)",syscall(SYS_unshare,CLONE_NEWUSER),EINVAL);
  chk("seg","unshare(CLONE_NEWNS)",syscall(SYS_unshare,CLONE_NEWNS),EPERM);
  chk("seg","unshare(0)",syscall(SYS_unshare,0),0);
  chk("seg","setns(0,0)",syscall(SYS_setns,0,0),EINVAL);
  chk("seg","chroot(/tmp)",syscall(SYS_chroot,"/tmp"),EPERM);
  chk("seg","mount(tmpfs)",syscall(SYS_mount,"none","/","tmpfs",0,0),EPERM);
  chk("seg","umount2",syscall(SYS_umount2,"/",0),EPERM);
  chk("seg","prctl(PR_SET_NO_NEW_PRIVS)",prctl(PR_SET_NO_NEW_PRIVS,1,0,0,0),0);
  // perdas de semantica que permanecem (devem aparecer como DIFERE no Pulsar)
  char f[64]; snprintf(f,sizeof f,"%s/a",d); int fd=open(f,O_CREAT|O_RDWR|O_TRUNC,0644); write(fd,"xxxx",4);
  struct stat st; fchmod(fd,0600); fstat(fd,&st); sem("fchmod altera o modo",(st.st_mode&0777)==0600);
  struct timespec ts[2]={{1000000,0},{1000000,0}}; utimensat(AT_FDCWD,f,ts,0); stat(f,&st); sem("utimensat altera mtime",st.st_mtime==1000000);
  char*m=mmap(0,4096,PROT_READ|PROT_WRITE,MAP_SHARED,fd,0);
  if(m!=MAP_FAILED){ m[0]='Y'; msync(m,4096,MS_SYNC); char b=0; pread(fd,&b,1,0); sem("MAP_SHARED+msync chega ao arquivo",b=='Y'); munmap(m,4096);} else sem("MAP_SHARED de arquivo",0);
  flock(fd,LOCK_EX);
  pid_t p=fork(); if(p==0){ int f2=open(f,O_RDWR); int rr=flock(f2,LOCK_EX|LOCK_NB); _exit(rr<0&&errno==EWOULDBLOCK?0:1);} int s; waitpid(p,&s,0);
  sem("flock exclusivo entre processos",WIFEXITED(s)&&WEXITSTATUS(s)==0);
  struct flock fl={.l_type=F_WRLCK,.l_whence=SEEK_SET,.l_start=0,.l_len=0}; fcntl(fd,F_SETLK,&fl);
  p=fork(); if(p==0){ int f2=open(f,O_RDWR); struct flock g={.l_type=F_WRLCK,.l_whence=SEEK_SET}; int rr=fcntl(f2,F_SETLK,&g); _exit(rr<0&&(errno==EAGAIN||errno==EACCES)?0:1);} waitpid(p,&s,0);
  sem("fcntl F_SETLK exclusivo entre processos",WIFEXITED(s)&&WEXITSTATUS(s)==0);
  struct rlimit rl={8,8}; setrlimit(RLIMIT_NOFILE,&rl); int last=0,k; for(k=0;k<16;k++){int x=open("/dev/null",O_RDONLY); if(x<0)break; last=x;}
  sem("setrlimit(NOFILE) e aplicado",k<16);
  printf("END stubtest diferencas=%d\n",fails);
  return 0;
}
