import os
import re,glob,statistics as st,collections,json
d=collections.defaultdict(lambda: collections.defaultdict(list))
order=[]
for f in sorted(glob.glob(os.environ['BENCH_DIR']+'/camp/g*_*_b*.log')):
    k=re.search(r'_(pulsar|linux)_b',f).group(1)
    for l in open(f,errors='ignore'):
        m=re.search(r'BENCH (\S+)\s+iters=\d+ mediana_ns=([\d.]+)',l)
        if m:
            n=m.group(1); d[n][k].append(float(m.group(2)))
            if n not in order: order.append(n)
def fmt(ns): return f"{ns/1000:.2f} µs" if ns<1e6 else f"{ns/1e6:.2f} ms"
rows=[]
print(f"{'op':20} {'Pulsar':>22} {'Linux':>22} {'P/L':>6}  n")
for n in order:
    p,L=d[n]['pulsar'],d[n]['linux']
    if not p or not L: continue
    mp,ml=st.median(p),st.median(L)
    sp=(max(p)-min(p))/mp*100; sl=(max(L)-min(L))/ml*100
    print(f"{n:20} {fmt(mp):>12} ±{sp:4.0f}% {fmt(ml):>12} ±{sl:4.0f}% {mp/ml:6.2f}  {len(p)}/{len(L)}")
    rows.append(dict(op=n,pulsar=p,linux=L))
json.dump(rows,open(os.environ['BENCH_DIR']+'/camp/summary.json','w'),indent=1)
