//! ball.pulse — demo grafico que desenha uma bola quicando na SUA
//! superficie. Fala com o WM via IPC (HELLO/COMMIT/POLL).
#![no_std]
#![no_main]
use plib::*;
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { exit() }
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! { run(); exit() }

fn wm()->usize{ loop{ let p=lookup_service(SVC_WM); if p>=0{return p as usize;} yield_now(); } }

fn run(){
    write("[ball] iniciando (superficie)\n");
    let wm=wm();
    let mut m=Msg::new(WM_OP_HELLO); m.data[0]=2;
    send(wm,&mut m);
    let slot=m.data[0]; let cw=m.data[1] as u32; let ch=m.data[2] as u32;
    let surf=surface_open(slot);

    let mut bx=(cw/2) as i32; let mut by=(ch/2) as i32;
    let mut vx=4i32; let mut vy=3i32; let r=22i32;

    loop{
        // POLL fechar
        let mut pm=Msg::new(WM_OP_POLL); send(wm,&mut pm);
        if pm.data[0]&WM_FLAG_CLOSE!=0{ exit(); }

        // fisica
        bx+=vx; by+=vy;
        if bx-r<0||bx+r>cw as i32{ vx=-vx; bx+=vx; }
        if by-r<0||by+r>ch as i32{ vy=-vy; by+=vy; }

        // desenha na superficie
        surf.vgrad(Rect{x:0,y:0,w:cw,h:ch},0x00101828,0x001A2942);
        // rastro
        surf.disc(bx as u32,by as u32,r as u32,0x00FF8040);
        surf.disc((bx-vx*2)as u32,(by-vy*2)as u32,(r-6)as u32,0x00FF9F0A);
        surf.text_ui(12,12,b"Ball demo - superficie propria",0x00A8C4E8);

        let mut cm=Msg::new(WM_OP_COMMIT); cm.data[0]=slot; send(wm,&mut cm);
        yield_now();
    }
}
