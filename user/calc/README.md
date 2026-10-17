# Calculadora (C)

Calculadora no estilo macOS, cliente do WM com superfície própria.

- Ponto fixo com 6 casas decimais e formatação brasileira (`1.234,56`).
- Operações `+ − × ÷`, `%`, troca de sinal, `AC`; Backspace apaga o último dígito.
- Teclado completo: dígitos, `+ - * /`, `,`/`.`, Enter/`=`, Esc.
- Realce ao passar o mouse e ao pressionar; segue o tema claro/escuro.
- Menu da barra: **Calcular > Limpar**.

Compilada pelo `Makefile` da raiz (`make`, ou `make ARCH=riscv64`).
