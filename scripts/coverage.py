import os
import re,sys,json,collections
src=open(os.environ['PULSAR_DIR']+'/kernel/src/linux.rs').read()
# tabela asm-generic (aarch64) da musl
tab={}
for l in open(os.environ['PULSAR_DIR']+'/user/third_party/musl/arch/aarch64/bits/syscall.h.in'):
    m=re.match(r'#define __NR_(\w+)\s+(\d+)',l)
    if m: tab[int(m.group(2))]=m.group(1)
def body(fn):
    i=src.index(fn); j=src.index('{',i); d=0
    for k in range(j,len(src)):
        d+= src[k]=='{'; d-= src[k]=='}'
        if d==0: return src[j:k]
arms={}
for fn,kind in [('pub fn dispatch_nr','dispatch'),('fn call(frame','call')]:
    b=body(fn); depth=0; lines=b.split('\n')
    # captura braços de nivel 2 (dentro do match principal)
    cur=None
    for idx,l in enumerate(lines):
        if depth==2:
            m=re.match(r'\s*((?:0x[0-9a-fA-F]+|\d+)(?:\s*\|\s*(?:0x[0-9a-fA-F]+|\d+))*)\s*(if [^=]*)?=>\s*(.*)',l)
            if m:
                nums=[int(x,0) for x in re.split(r'\s*\|\s*',m.group(1))]
                rest=m.group(3); cfg=lines[idx-1].strip() if idx else ''
                for n in nums:
                    arms.setdefault(n,(kind,rest.strip(),cfg))
        depth+=l.count('{')-l.count('}')
def classify(rest):
    r=re.sub(r'//.*','',rest).strip().rstrip(',')
    if re.fullmatch(r'Err\((ENOSYS|EOPNOTSUPP|EINVAL|EPERM)\)',r): return 'stub-erro'
    if re.fullmatch(r'Ok\(0\)',r): return 'stub-ok'
    return 'implementada'
cnt=collections.Counter(); out=[]
for n,(k,rest,cfg) in sorted(arms.items()):
    if n not in tab: continue
    if 'riscv64' in cfg or 'x86_64' in cfg: continue
    c=classify(rest); cnt[c]+=1; out.append((n,tab[n],c))
print('syscalls asm-generic (aarch64) na tabela da musl:',len(tab))
print('tratadas pelo linux.rs:',sum(cnt.values()),dict(cnt))
print('nao tratadas (-ENOSYS):',len(tab)-sum(cnt.values()))
print('\nstubs:'); print(', '.join(f'{nm}({n})[{c}]' for n,nm,c in out if c!='implementada'))
handled={n for n,_,_ in out}
json.dump({'table':tab,'arms':[(n,nm,c) for n,nm,c in out]},open(os.environ['BENCH_DIR']+'/coverage.json','w'))
