#!/usr/bin/env python3
"""bin2c.py — embute arquivos binarios num fonte C.

Uso: bin2c.py <saida.c> nome=arquivo [nome=arquivo ...]

Para cada par gera:
    const unsigned char nome[] = { ..., 0 };   (com um NUL extra no fim)
    const unsigned int nome_len = <tamanho sem o NUL>;
O NUL extra deixa textos (como o prelude.js) prontos para o QuickJS.
"""
import sys

def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    out = ['/* GERADO por tools/bin2c.py. NAO EDITAR. */']
    for spec in sys.argv[2:]:
        name, path = spec.split('=', 1)
        data = open(path, 'rb').read() + b'\0'
        out.append(f'const unsigned char {name}[] __attribute__((aligned(8))) = {{')
        for i in range(0, len(data), 24):
            out.append(','.join(str(b) for b in data[i:i + 24]) + ',')
        out.append('};')
        out.append(f'const unsigned int {name}_len = {len(data) - 1};')
    open(sys.argv[1], 'w').write('\n'.join(out) + '\n')

main()
