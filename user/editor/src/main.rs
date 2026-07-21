//! editor.pulse — editor de texto que desenha na SUA superficie (nao no
//! framebuffer direto). Fala com o WM via IPC: HELLO (pega slot+geometria),
//! COMMIT (avisa que desenhou), POLL (recebe evento de fechar).
#![no_std]
#![no_main]
use plib::*;
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { write("[editor] PANIC\n"); exit() }
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! { run(); exit() }

const MAXLEN:usize=2048;
const KEY_F1:u16=59; const KEY_F2:u16=60; const KEY_ESC:u16=1;

fn wm()->usize{ loop{ let p=lookup_service(SVC_WM); if p>=0{return p as usize;} yield_now(); } }

fn run(){
    write("[editor] iniciando (superficie)\n");
    let wm=wm();
    // HELLO: pega slot + geometria da area cliente
    let mut m=Msg::new(WM_OP_HELLO); m.data[0]=1;
    send(wm,&mut m);
    let slot=m.data[0]; let cw=m.data[1] as u32; let ch=m.data[2] as u32;
    let surf=surface_open(slot);

    // abre welcome.txt via IPC do fsd
    let dd=loop{let p=lookup_service(SVC_FS); if p>=0{break p as usize;} yield_now();};
    let mut buf=[0u8;MAXLEN]; let mut len=0usize; let mut file_idx:i64=-1;
    // acha welcome.txt
    let n={let mut mm=Msg::new(FS_OP_COUNT); send(dd,&mut mm); mm.data[0] as usize};
    for i in 0..n{
        let mut mm=Msg::new(FS_OP_STAT); mm.data[0]=i as u64; send(dd,&mut mm);
        let mut name=[0u8;32];
        for w in 0..4{let word=mm.data[2+w];for b in 0..8{name[w*8+b]=(word>>(b*8))as u8;}}
        let ne=name.iter().position(|&x|x==0).unwrap_or(32);
        if &name[..ne]==b"welcome.txt"{ file_idx=i as i64; break; }
    }
    // le conteudo via fs_read direto (o editor tem essa syscall)
    if file_idx>=0{ let got=fs_read(file_idx as usize,&mut buf); if got>0{len=(got as usize).min(MAXLEN);} }
    write("[editor] arquivo carregado\n");

    let mut saved_flash=0u32; let mut blink=0u32;

    loop{
        // input (teclado, roteado pelo foco)
        for _ in 0..64{
            let Some(ev)=input_poll() else {break};
            if ev.ev_type==EV_KEY && ev.value==1{
                match ev.code{
                    KEY_F1|KEY_F2=>{ if file_idx>=0 && fs_write(file_idx as usize,&buf[..len])==0{ saved_flash=60; write("[editor] salvo\n"); } }
                    KEY_ESC=>{ set_focus(0); exit(); }
                    KEY_ENTER=>{ if len<MAXLEN{buf[len]=b'\n';len+=1;} }
                    KEY_BACKSPACE=>{ if len>0{len-=1;} }
                    _=>{ let c=keycode_to_lower(ev.code); if c!=0 && len<MAXLEN{buf[len]=c;len+=1;} }
                }
            }
        }

        // POLL: o WM quer que eu feche?
        let mut pm=Msg::new(WM_OP_POLL); send(wm,&mut pm);
        if pm.data[0]&WM_FLAG_CLOSE!=0{ set_focus(0); exit(); }

        // desenha na SUPERFICIE (coordenadas locais 0..cw,0..ch)
        blink=blink.wrapping_add(1);
        surf.round_rect(Rect{x:0,y:0,w:cw,h:ch},0,0x00FCFDFE);
        // barra de status
        surf.fill(Rect{x:0,y:0,w:cw,h:22},0x00EEF1F7);
        surf.text_ui(10,3,b"welcome.txt",0x00579BFF);
        if saved_flash>0{ surf.text_ui(cw as i32-64,3,b"Salvo",0x0032D256); saved_flash-=1; }
        else{ surf.text_ui_a(cw as i32-150,3,b"F2 salva - ESC sai",0x00909AAC,220); }
        // texto com wrap
        let tx0=10i32; let ty0=30i32; let cols=((cw-20)/8).max(1) as usize;
        let mut cx=0usize; let mut cy=0i32;
        for i in 0..len{
            let ch2=buf[i];
            if ch2==b'\n'||cx>=cols{ cx=0; cy+=20; if ch2==b'\n'{continue;} }
            if (ty0+cy+16) < ch as i32{ let one=[ch2]; surf.text_ui(tx0+(cx as i32)*8,ty0+cy,&one,0x001E2530); }
            cx+=1;
        }
        // cursor de texto
        let curx=tx0+((cx%cols)as i32)*8; let cury=ty0+cy;
        if (blink/8)%2==0 && (cury+16)<ch as i32{ surf.fill(Rect{x:curx as u32,y:cury as u32,w:2,h:16},0x00579BFF); }

        // COMMIT: avisa o WM que desenhei
        let mut cm=Msg::new(WM_OP_COMMIT); cm.data[0]=slot; send(wm,&mut cm);
        yield_now();
    }
}
