import re,sys,json,collections
ansi=re.compile(r'\x1b\[[0-9;]*m')
def parse(f):
    res={}; cur=None; buf=[]
    for l in open(f,errors='ignore'):
        l=ansi.sub('',l).rstrip('\r\n')
        m=re.match(r'LTP-BEGIN (\S+)',l)
        if m: cur=m.group(1); buf=[]; continue
        m=re.match(r'LTP-RESULT (\S+) (\S+) code=(-?\d+)',l)
        if m and cur:
            tag,res0=m.group(1),m.group(2); txt='\n'.join(buf)
            if tag.startswith('df'): cur=None; continue
            if res0 in ('TIMEOUT','CRASH','NOEXEC'): r=res0
            elif re.search(r'\bTBROK\b',txt): r='BROK'
            elif re.search(r'\bTFAIL\b',txt): r='FAIL'
            elif re.search(r'\bTPASS\b',txt): r='PASS'
            elif re.search(r'\bTCONF\b',txt): r='CONF'
            else: r={'PASS':'NORESULT'}.get(res0,res0)
            msgs=[x for x in buf if re.search(r'\bT(BROK|FAIL|CONF)\b',x)][:3]
            res[tag]=(r,msgs); cur=None; continue
        if cur: buf.append(l)
    return res
if __name__=='__main__':
    P=parse(sys.argv[1]); L=parse(sys.argv[2])
    print('n',len(P),len(L))
    print('Pulsar',collections.Counter(v[0] for v in P.values()))
    print('Linux ',collections.Counter(v[0] for v in L.values()))
    json.dump({'P':P,'L':L},open(sys.argv[3],'w'),indent=0)
