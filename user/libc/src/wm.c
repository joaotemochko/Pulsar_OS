/* wm.c — superfícies por cliente, input decodificado, protocolo WM. */
#include "pulsar.h"

Fb surface_open(u64 slot){
    Fb fb; fb.base=(u32*)sys_surf_map(slot); fb.w=SURF_W; fb.h=SURF_H; return fb;
}
InputEv input_poll_ev(void){
    u64 v=sys_input_poll();
    InputEv e;
    e.valid=(v>>63)&1;
    e.type=(u16)((v>>48)&0x7FFF);
    e.code=(u16)((v>>32)&0xFFFF);
    e.value=(i32)(v&0xFFFFFFFF);
    return e;
}
void wm_hello(u64 wm, u64 kind, u64* slot, u32* cw, u32* ch){
    Msg m; memset(&m,0,sizeof m); m.tag=WM_OP_HELLO;
    m.data[0]=kind; m.data[1]=*cw; m.data[2]=*ch;
    ipc_send(wm,&m);
    *slot=m.data[0]; *cw=(u32)m.data[1]; *ch=(u32)m.data[2];
}
void wm_commit(u64 wm, u64 slot){
    Msg m; memset(&m,0,sizeof m); m.tag=WM_OP_COMMIT; m.data[0]=slot;
    ipc_send(wm,&m);
}
WmEvent wm_poll(u64 wm){
    Msg m; memset(&m,0,sizeof m); m.tag=WM_OP_POLL;
    ipc_send(wm,&m);
    WmEvent e;
    e.close=(m.data[0]&WM_FLAG_CLOSE)!=0;
    e.focused=(m.data[0]&WM_FLAG_FOCUS)!=0;
    e.mx=(i32)m.data[1]; e.my=(i32)m.data[2];
    e.click=(m.data[3]&WM_MOUSE_CLICK)!=0;
    e.key=(u16)m.data[4];
    e.menu=(u16)m.data[5];
    return e;
}
