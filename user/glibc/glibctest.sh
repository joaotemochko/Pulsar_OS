#!/usr/bin/bash
# glibctest.sh — programas Linux prontos (Debian, glibc) no Pulsar: bash e
# coreutils sem nenhuma modificacao. Cada linha compara com o esperado.
falhas=0
t() {  # t <descricao> <esperado> <obtido>
    if [ "$2" = "$3" ]; then echo "[glibctest] ok    $1"; else echo "[glibctest] FALHA $1: esperado '$2', veio '$3'"; falhas=$((falhas+1)); fi
}
t "bash: aritmetica e variaveis" "42" "$(( 6 * 7 ))"
t "pipe entre processos (seq | tail | paste)" "8 9 10" "$(/usr/bin/seq 10 | /usr/bin/tail -n 3 | /usr/bin/paste -sd ' ')"
t "sort -n e uniq" "1 2 3" "$(printf '3\n1\n2\n3\n' | /usr/bin/sort -n | /usr/bin/uniq | /usr/bin/paste -sd ' ')"
t "sha256sum" "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824" "$(printf hello | /usr/bin/sha256sum | /usr/bin/cut -d' ' -f1)"
t "UTF-8 (wc -m)" "4" "$(printf 'Ação' | LANG=C.UTF-8 /usr/bin/wc -m)"
t "uname" "Linux" "$(/usr/bin/uname -s)"
t 'subshell e $?' "7" "$( (exit 7); echo $? )"
f=$(/usr/bin/mktemp)
t "arquivo temporario (mktemp, cat)" "conteudo" "$(echo conteudo > "$f"; /usr/bin/cat "$f")"
/usr/bin/rm -f "$f"; r=$?
t "rm apaga o arquivo" "0 nao" "$r $([ -e "$f" ] && echo sim || echo nao)"
t "ls conta os programas do Debian" "sim" "$([ $(/usr/bin/ls /usr/bin | /usr/bin/wc -l) -gt 50 ] && echo sim)"
t "df ve o ext2 do Pulsar" "/" "$(/usr/bin/df --output=target / | /usr/bin/tail -1)"
inicio=$(/usr/bin/date +%s%N); /usr/bin/sleep 0.2; fim=$(/usr/bin/date +%s%N)
t "sleep 0.2 (date em ns)" "sim" "$([ $(( (fim - inicio) / 1000000 )) -ge 190 ] && echo sim)"
echo "[glibctest] $(/usr/bin/uname -a)"
echo "[glibctest] fim: $falhas falha(s)"
