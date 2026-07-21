# Calculadora do Pulsar (em C)

Segundo app funcional em C (depois do terminal). Cliente do Window Manager:
superficie propria, recebe cliques locais (ev.mx/ev.my) e desenha um teclado
no estilo macOS (operadores em laranja, numeros em cinza).

## Recursos
- Aritmetica inteira: + - * / , troca de sinal (+/-), porcentagem, limpar (C).
- Roteamento de clique do WM para os botoes.
- Integra com a barra de menu por-programa: menu "Calcular > Limpar".
- Display alinhado a direita com a fonte AA Poppins.

## Compilar
```
cd user/calc && ./build.sh
python3 tools/mkpulse.py calc.elf calc.pulse 0x12400000
```
