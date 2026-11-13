export PATH=/usr/bin:/bin
pass=0; fail=0
t(){ if eval "$2" >/dev/null 2>&1; then pass=$((pass+1)); echo "GT ok    $1"; else fail=$((fail+1)); echo "GT FALHA $1"; fi; }
t echo '[ "$(echo hello)" = hello ]'
t pipe-sort '[ "$(printf "b\na\nc\n" | sort | head -1)" = a ]'
t seq-wc '[ "$(seq 1 1000 | wc -l)" = 1000 ]'
t sha256sum '[ "$(printf abc | sha256sum | cut -c1-8)" = ba7816bf ]'
t mkdir-cp-mv-rm 'mkdir -p /tmp/g/d && echo x > /tmp/g/d/f && cp /tmp/g/d/f /tmp/g/f2 && mv /tmp/g/f2 /tmp/g/f3 && [ -f /tmp/g/f3 ] && rm -r /tmp/g && [ ! -e /tmp/g ]'
t ln-readlink 'ln -s /etc/hosts /tmp/lnk && [ "$(readlink /tmp/lnk)" = /etc/hosts ] && rm /tmp/lnk'
t stat 'echo abc > /tmp/st && [ "$(stat -c %s /tmp/st)" = 4 ]'
t date 'date +%s'
t id '[ "$(id -u)" = 1000 ]'
t subshell-status '[ "$( (exit 3); echo $? )" = 3 ]'
t background-wait 'sleep 0.2 & p=$!; wait $p'
t two-jobs-status '(sleep 0.1; exit 4) & a=$!; (exit 5) & b=$!; wait $a; ra=$?; wait $b; rb=$?; [ $ra = 4 ] && [ $rb = 5 ]'
t trap-signal 'got=0; trap "got=1" USR1; kill -USR1 $$; [ $got = 1 ]'
t kill-child 'sleep 5 & p=$!; kill $p; wait $p; [ $? -gt 128 ]'
t heredoc '[ "$(cat <<EOT | tail -1
x
y
EOT
)" = y ]'
t env '[[ "$(X=1 env)" == *$'"'"'\n'"'"'X=1$'"'"'\n'"'"'* || "$(X=1 env)" == X=1$'"'"'\n'"'"'* ]]'
t expr '[ "$(expr 6 \* 7)" = 42 ]'
t arith-loop 'n=0; for i in $(seq 1 200); do n=$((n+i)); done; [ $n = 20100 ]'
t sort-20000 '[ "$(seq 1 20000 | sort -rn | head -1)" = 20000 ]'
t tail 'printf "a\nb\n" > /tmp/t1; [ "$(tail -1 /tmp/t1)" = b ]'
t uname '[ "$(uname -m)" = aarch64 ]'
t nproc 'nproc'
t ls 'ls -la /'
t pipeline-3 '[ "$(seq 1 100 | sort -rn | head -3 | tail -1)" = 98 ]'
t chmod 'touch /tmp/cm; chmod 600 /tmp/cm; [ "$(stat -c %a /tmp/cm)" = 600 ]'
t touch-mtime 'touch -d @1000000 /tmp/tm; [ "$(stat -c %Y /tmp/tm)" = 1000000 ]'
echo "GLIBCTEST pass=$pass fail=$fail"
