/* paint.c — display list -> pixels da janela, com rolagem e recortes. */
#include "vela.h"

static void border(Fb* fb, const DItem* d, i32 x, i32 y){
    if(d->radius > 0 && d->bw[0] == d->bw[1] && d->bw[1] == d->bw[2] && d->bw[2] == d->bw[3]
       && d->bc[0] == d->bc[1] && d->bc[1] == d->bc[2] && d->bc[2] == d->bc[3] && d->bw[0] <= 2){
        i32 r = d->radius > d->h / 2 ? d->h / 2 : d->radius;
        if(r > d->w / 2) r = d->w / 2;
        for(int k = 0; k < d->bw[0]; k++)
            fb_round_border(fb, rect(x + k, y + k, d->w - 2 * k, d->h - 2 * k), r > k ? r - k : 0, d->bc[0] & 0xFFFFFF, d->bc[0] >> 24);
        return;
    }
    if(d->bw[0]) fb_fill_a(fb, rect(x, y, d->w, d->bw[0]), d->bc[0] & 0xFFFFFF, d->bc[0] >> 24);
    if(d->bw[2]) fb_fill_a(fb, rect(x, y + d->h - d->bw[2], d->w, d->bw[2]), d->bc[2] & 0xFFFFFF, d->bc[2] >> 24);
    if(d->bw[3]) fb_fill_a(fb, rect(x, y + d->bw[0], d->bw[3], d->h - d->bw[0] - d->bw[2]), d->bc[3] & 0xFFFFFF, d->bc[3] >> 24);
    if(d->bw[1]) fb_fill_a(fb, rect(x + d->w - d->bw[1], y + d->bw[0], d->bw[1], d->h - d->bw[0] - d->bw[2]), d->bc[1] & 0xFFFFFF, d->bc[1] >> 24);
}

void paint(Fb* fb, DList* dl, Rect view, Rect clip, i32 scroll, u32 page_bg){
    Fb f = *fb;
    i32 ox = view.x, oy = view.y;         /* origem da pagina na janela */
    view = rect_and(view, clip);          /* `view` passa a ser so a area repintada */
    fb_set_clip(&f, view);
    fb_fill(&f, view, page_bg & 0xFFFFFF);
    /* 2 passadas: o conteudo que rola, depois os position:fixed por cima */
    for(int pass = 0; pass < 2; pass++){
    Rect clips[32]; int nclip = 0;
    Rect cur = view;
    fb_set_clip(&f, view); cur = f.clip;
    for(int i = 0; i < dl->n; i++){
        DItem* d = &dl->it[i];
        if(d->fixed != pass) continue;
        i32 x = ox + d->x;
        i32 y = oy + d->y - (d->fixed ? 0 : scroll);
        switch(d->kind){
        case DL_CLIP:
            if(nclip < 32) clips[nclip++] = cur;
            cur = rect_and(cur, rect(x, y, d->w, d->h));
            f.clip = cur;
            continue;
        case DL_UNCLIP:
            if(nclip) cur = clips[--nclip];
            f.clip = cur;
            continue;
        case DL_HIT: continue;
        }
        /* fora da area visivel */
        if(y > cur.y + cur.h || y + d->h < cur.y || x > cur.x + cur.w || x + d->w < cur.x) continue;
        switch(d->kind){
        case DL_RECT: {
            u32 a = d->color >> 24;
            if(!a) break;
            if(d->radius > 0){
                i32 r = d->radius > d->h / 2 ? d->h / 2 : d->radius;
                if(r > d->w / 2) r = d->w / 2;
                fb_round_rect_a(&f, rect(x, y, d->w, d->h), r, d->color & 0xFFFFFF, a);
            } else fb_fill_a(&f, rect(x, y, d->w, d->h), d->color & 0xFFFFFF, a);
            break;
        }
        case DL_BORDER: border(&f, d, x, y); break;
        case DL_TEXT: font_draw(&f, d->font, x, y + font_ascent(d->font), d->s, d->n, d->color, d->ls); break;
        case DL_IMAGE: image_draw(&f, d->img, rect(x, y, d->w, d->h), d->alpha, d->radius == 1); break;
        case DL_GRAD: grad_draw(&f, d->grad, rect(x, y, d->w, d->h), d->radius, d->alpha); break;
        case DL_BGIMAGE: case DL_MASK:
            image_draw_tiled(&f, d->img, rect(x, y, d->w, d->h), x + d->ix, y + d->iy, d->iw, d->ih, d->rep, d->alpha, d->kind == DL_MASK, d->color);
            break;
        case DL_CARET: fb_fill(&f, rect(x, y, 1, d->h), d->color & 0xFFFFFF); break;
        }
    }
    }
}
