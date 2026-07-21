/* hello_c — primeiro app em C do Pulsar OS. Desenha um gradiente e
 * alguns retângulos para provar que a toolchain C + libc funcionam. */
#include "pulsar.h"

int main(void){
    puts_("[hello_c] app em C iniciando\n");
    Fb fb = fb_open();

    /* fundo: gradiente vertical azul */
    for(u32 y=0; y<fb.h; y++){
        u32 t = (y*255)/fb.h;
        u32 color = (0x20<<16) | ((0x33+t/4)<<8) | (0x58+t/3);
        fb_fill(&fb, 0, y, fb.w, 1, color);
    }

    /* barras coloridas centrais */
    u32 colors[5] = {0xFF5F57, 0xFEBC2E, 0x28C840, 0x579BFF, 0xAF6BFF};
    u32 bw = fb.w/6;
    for(int i=0; i<5; i++){
        u32 bx = bw/2 + (u32)i*bw + bw/4;
        for(u32 yy=0; yy<200; yy++)
            for(u32 xx=0; xx<bw-20; xx++)
                fb_blend(&fb, bx+xx, fb.h/2-100+yy, colors[i], 230);
    }

    /* quadro branco translúcido no topo */
    for(u32 yy=0; yy<80; yy++)
        for(u32 xx=0; xx<fb.w; xx++)
            fb_blend(&fb, xx, yy, 0xFFFFFF, 40);

    puts_("[hello_c] desenho pronto, apresentando\n");
    for(;;){
        sys_fb_present();
        sys_yield();
    }
    return 0;
}
