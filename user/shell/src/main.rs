//! Pulsar WM — compositor com multiplas janelas de app, barra de menu
//! interativa (dropdowns), dock, e sistema de arquivos que abre apps.
#![no_std]
#![no_main]
use plib::*;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { write("[wm] PANIC\n"); exit() }
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! { main(); exit() }

const WALL_TOP:u32=0x00203358; const WALL_BOT:u32=0x00355789; const AURORA:u32=0x005A8AD0;
const GLASS:u32=0x00FAFBFE; const GLASS_BOT:u32=0x00EDF0F6;
const INK:u32=0x001E2530; const INK_DIM:u32=0x007C8697; const WHITE:u32=0x00FFFFFF;
const MENU_H:u32=32; const DOCK_H:u32=76; const TITLE_H:u32=40;
const MAXWIN:usize=6;

fn fsd()->i64{ lookup_service(SVC_FS) }
fn fs_count_ipc(dd:usize)->u32{ let mut m=Msg::new(FS_OP_COUNT); send(dd,&mut m); m.data[0] as u32 }
fn fs_stat_ipc(dd:usize,i:usize,name:&mut[u8;32])->(i64,u32){
    let mut m=Msg::new(FS_OP_STAT); m.data[0]=i as u64; send(dd,&mut m);
    for w in 0..4{let word=m.data[2+w];for b in 0..8{name[w*8+b]=(word>>(b*8))as u8;}}
    (m.data[0] as i64, m.data[1] as u32)
}
fn u32_dec(mut v:u32,out:&mut[u8;10])->usize{ if v==0{out[9]=b'0';return 9;} let mut i=10; while v>0{i-=1;out[i]=b'0'+(v%10)as u8;v/=10;} i }

#[derive(Clone,Copy,PartialEq)]
enum WKind{ Files, App }
#[derive(Clone,Copy)]
struct Win{
    used:bool, kind:WKind, r:Rect, saved:Rect, open:bool, max:bool, z:u8,
    pid:i64, slot:u8, app_kind:u8, committed:bool, want_close:bool,
    // eventos pendentes para o app
    ev_mx:i32, ev_my:i32, ev_click:bool, ev_key:u16, ev_menu:u16,
}
impl Win{
    const fn empty()->Self{ Win{used:false,kind:WKind::App,r:Rect{x:0,y:0,w:0,h:0},saved:Rect{x:0,y:0,w:0,h:0},open:false,max:false,z:0,pid:-1,slot:255,app_kind:0,committed:false,want_close:false,ev_mx:-1,ev_my:-1,ev_click:false,ev_key:0,ev_menu:0} }
    fn titlebar(&self)->Rect{ Rect{x:self.r.x,y:self.r.y,w:self.r.w,h:TITLE_H} }
    fn client(&self)->Rect{ Rect{x:self.r.x,y:self.r.y+TITLE_H,w:self.r.w,h:self.r.h-TITLE_H} }
    fn b_close(&self)->Rect{ Rect{x:self.r.x+16,y:self.r.y+12,w:18,h:18} }
    fn b_max(&self)->Rect{ Rect{x:self.r.x+64,y:self.r.y+12,w:18,h:18} }
}

// apps do dock/launcher. bin vazio = janela interna (arquivos)
struct AppDef{color:u32,glyph:u8,name:&'static[u8],bin:&'static str,kind:u8,w:u32,h:u32}

fn title_for(kind:u8)->&'static[u8]{
    match kind{ 1=>b"Editor", 2=>b"Ball", 3=>b"Calculadora", 4=>b"Sobre", 5=>b"Terminal", 6=>b"Rede", 7=>b"Vela", _=>b"App" }
}

fn main(){
    write("[wm] Pulsar WM (multi-janela) iniciando\n");
    let mut fb=Fb::open();
    let (w,h)=(fb.w,fb.h);
    register_service(SVC_WM);
    let dd=loop{let p=fsd(); if p>=0{break p as usize;} yield_now();};
    write("[wm] fsd conectado, SVC_WM registrado\n");
    set_focus(getpid()); // WM recebe o teclado e encaminha aos apps

    compose_wallpaper(&fb,w,h);
    fb.restore_wall(Rect{x:0,y:0,w,h});

    // catalogo de apps
    let apps=[
        AppDef{color:0x00579BFF,glyph:b'F',name:b"Arquivos",   bin:"",             kind:0, w:0,h:0},
        AppDef{color:0x0032D256,glyph:b'E',name:b"Editor",     bin:"editor.pulse", kind:1, w:500,h:440},
        AppDef{color:0x00FF9F0A,glyph:b'B',name:b"Ball",       bin:"ball.pulse",   kind:2, w:360,h:360},
        AppDef{color:0x00AF6BFF,glyph:b'C',name:b"Calculadora",bin:"calc.pulse",   kind:3, w:280,h:400},
        AppDef{color:0x00FF6482,glyph:b'S',name:b"Sobre",      bin:"about.pulse",  kind:4, w:420,h:300},
        AppDef{color:0x0030C8D0,glyph:b'T',name:b"Terminal",   bin:"terminal.pulse",kind:5, w:560,h:380},
        AppDef{color:0x005AC8FF,glyph:b'N',name:b"Rede",       bin:"netmon.pulse", kind:6, w:420,h:340},
        AppDef{color:0x00FF7A59,glyph:b'V',name:b"Vela",       bin:"vela.pulse",   kind:7, w:720,h:520},
    ];
    let n_apps=apps.len() as u32;
    let icon=52u32; let gap=18u32;
    let dock_w=n_apps*icon+(n_apps+1)*gap;
    let dock_x=w/2-dock_w/2; let dock_y=h-DOCK_H-12;

    let mut wins=[Win::empty();MAXWIN];
    // janela 0 = arquivos (sempre presente)
    wins[0]=Win{used:true,kind:WKind::Files,r:Rect{x:80,y:MENU_H+40,w:440,h:400},saved:Rect{x:80,y:MENU_H+40,w:440,h:400},open:false,max:false,z:1,pid:-1,slot:255,app_kind:0,committed:true,want_close:false,ev_mx:-1,ev_my:-1,ev_click:false,ev_key:0,ev_menu:0};
    let mut zmax=1u8;
    let mut next_slot=0u8;

    let mut cursor=Cursor::new(w/2,h/2);
    let mut mouse=Mouse::new(w/2,h/2);
    let mut fcache:[FileMeta;9]=[FileMeta{name:[0;32],nlen:0,size:0,kind:0};9];
    let mut nfiles=fs_count_ipc(dd);
    refresh_files(dd,nfiles,&mut fcache);
    let mut tick=0u32;
    let mut bounce=[0u32;8];
    let mut drag:i32=-1; let mut ddx=0i32; let mut ddy=0i32;
    let mut drag_prev_r=Rect{x:0,y:0,w:0,h:0}; // posicao da janela no frame anterior (p/ damage)
    let mut sel:i32=-1;
    let mut msg=Msg::new(0);
    let mut dirty=true; let mut last_mx=w/2; let mut last_my=h/2;
    // damage acumulado no frame (abrir/fechar/foco). None = sem dano especifico.
    // Quando Some(r), recompoe so r em vez da tela toda.
    let mut damage:Option<Rect>=None;
    let acc=|d:&mut Option<Rect>, r:Rect|{ *d=Some(match *d{ Some(o)=>rect_union(o,r), None=>r }); };
    let mut any_commit=false;
    let mut frame_start=uptime_ms();
    // menu dropdown aberto: -1 nenhum, senao indice do menu (0=app,1=arquivo,2=editar,3=janela)
    let mut menu_open:i32=-1;

    loop {
        // ---- atender apps (nao-bloqueante) ----
        for _ in 0..16 {
            let Some(from)=try_recv(&mut msg) else {break};
            let mut r=Msg::new(msg.tag);
            // acha a janela desse pid
            let wi=wins.iter().position(|x|x.used && x.pid==from as i64);
            match msg.tag {
                x if x==WM_OP_HELLO => {
                    // registra: acha a janela recem-criada sem pid ainda com este app
                    if let Some(idx)=wins.iter().position(|x|x.used && x.pid<0 && x.kind==WKind::App){
                        wins[idx].pid=from as i64;
                        let c=wins[idx].client();
                        r.data[0]=wins[idx].slot as u64;
                        r.data[1]=c.w as u64; r.data[2]=c.h as u64;
                    }
                }
                x if x==WM_OP_COMMIT => { if let Some(i)=wi{ wins[i].committed=true; any_commit=true; } }
                x if x==WM_OP_POLL => {
                    if let Some(i)=wi{
                        let mut flags=0u64;
                        if wins[i].want_close{flags|=WM_FLAG_CLOSE;}
                        if wins[i].z==zmax{flags|=WM_FLAG_FOCUS;}
                        r.data[0]=flags;
                        r.data[1]=wins[i].ev_mx as u64; r.data[2]=wins[i].ev_my as u64;
                        let mut mf=0u64; if wins[i].ev_click{mf|=WM_MOUSE_CLICK;}
                        r.data[3]=mf; r.data[4]=wins[i].ev_key as u64;
                        r.data[5]=wins[i].ev_menu as u64;  // acao de menu (0=nenhuma)
                        // consome os eventos
                        wins[i].ev_click=false; wins[i].ev_key=0; wins[i].ev_mx=-1; wins[i].ev_my=-1;
                        wins[i].ev_menu=0;
                    }
                }
                _=>{}
            }
            reply(from,&r);
        }

        // ---- entrada ----
        for _ in 0..256 {
            let Some(ev)=input_poll() else {break};
            mouse.feed(&ev,w,h);
            // teclado vai para o app focado (se houver)
            if ev.ev_type==EV_KEY && ev.value==1 && ev.code!=BTN_LEFT {
                if let Some(i)=focused_app(&wins,zmax){ wins[i].ev_key=ev.code; dirty=true; }
            }
        }
        cursor.x=mouse.x; cursor.y=mouse.y;
        mouse.update(uptime_ms());

        // arrasto
        if drag>=0 {
            if mouse.down {
                if !wins[drag as usize].max {
                    wins[drag as usize].r.x=(mouse.x as i32-ddx).max(0).min((w-60)as i32)as u32;
                    wins[drag as usize].r.y=(mouse.y as i32-ddy).max(MENU_H as i32).min((h-60)as i32)as u32;
                    dirty=true;
                }
            } else { drag=-1; }
        }

        if mouse.clicked {
            dirty=true;
            let my=mouse.y; let mx=mouse.x;
            // 1) barra de menu (dropdowns)?
            if my<MENU_H {
                menu_open = menu_hit(mx, &wins, zmax);
            } else if menu_open>=0 {
                // clique num item do dropdown aberto?
                let act=dropdown_hit(menu_open,mx,my,&wins,zmax);
                handle_menu_action(act,&mut wins,zmax);
                menu_open=-1;
            } else {
                menu_open=-1;
                // 2) janelas (frente->tras)
                let ord=z_desc(&wins);
                let mut done=false;
                for &wi in ord.iter(){
                    if wi>=MAXWIN {continue;}
                    let win=wins[wi];
                    if !win.used||!win.open {continue;}
                    if win.b_close().contains(mx,my){
                        if win.kind==WKind::App{wins[wi].want_close=true;}
                        else {
                            let r=wins[wi].r;
                            acc(&mut damage,Rect{x:r.x.saturating_sub(22),y:r.y.saturating_sub(22),w:r.w+44,h:r.h+44});
                            wins[wi].open=false;
                        }
                        done=true;break;
                    }
                    if win.b_max().contains(mx,my){ toggle_max(&mut wins[wi],w,h); done=true;break; }
                    if win.titlebar().contains(mx,my){
                        zmax+=1; wins[wi].z=zmax;
                        drag=wi as i32; ddx=mx as i32-win.r.x as i32; ddy=my as i32-win.r.y as i32;
                        done=true;break;
                    }
                    if win.r.contains(mx,my){
                        zmax+=1; wins[wi].z=zmax;
                        if win.kind==WKind::Files{
                            let idx=file_at(&win,mx,my,nfiles);
                            if idx>=0{ sel=idx;
                                if mouse.double{ open_file_by_ext(&mut wins,&apps,&fcache,idx,&mut zmax,&mut next_slot,&mut bounce); }
                            }
                        } else {
                            // entrega o clique ao app (coordenadas locais)
                            let c=win.client();
                            wins[wi].ev_mx=(mx-c.x) as i32; wins[wi].ev_my=(my-c.y) as i32; wins[wi].ev_click=true;
                        }
                        done=true;break;
                    }
                }
                if !done{
                    // 3) dock
                    for i in 0..n_apps{
                        let ix=dock_x+gap+i*(icon+gap);
                        if (Rect{x:ix,y:dock_y+12,w:icon,h:icon}).contains(mx,my){
                            let a=&apps[i as usize];
                            if a.bin.is_empty(){ wins[0].open=true; zmax+=1; wins[0].z=zmax; }
                            else { launch(&mut wins,a,&mut zmax,&mut next_slot,&mut bounce,i as usize); }
                        }
                    }
                }
            }
        }

        // liberar janelas de apps mortos
        for i in 0..MAXWIN{
            if wins[i].used && wins[i].kind==WKind::App && wins[i].pid>=0 && !is_alive(wins[i].pid as usize){
                // dano = area da janela morta (+ margem p/ sombra)
                let r=wins[i].r;
                let dr=Rect{x:r.x.saturating_sub(22),y:r.y.saturating_sub(22),w:r.w+44,h:r.h+44};
                acc(&mut damage,dr);
                wins[i]=Win::empty(); dirty=true; set_focus(getpid());
            }
        }

        // ---- damage: so recompoe quando algo VISUAL muda (nao no mouse-move puro) ----
        // Compositores reais nao redesenham a cena quando o cursor so anda:
        // movem o cursor via backing-store. Marcamos dirty apenas quando o
        // movimento cruza uma regiao interativa que muda de aparencia (hover
        // na lista de arquivos, hover no dock) ou quando ha arraste/animacao.
        let moved = mouse.x!=last_mx||mouse.y!=last_my;
        if moved {
            // hover sobre a janela Files (highlight de linha muda)?
            let over_files = wins[0].used && wins[0].open && wins[0].r.contains(mouse.x,mouse.y);
            let was_over_files = wins[0].used && wins[0].open && wins[0].r.contains(last_mx,last_my);
            // hover sobre o dock (icone acende)?
            let dock_r = Rect{x:dock_x,y:dock_y,w:dock_w,h:DOCK_H};
            let over_dock = dock_r.contains(mouse.x,mouse.y);
            let was_over_dock = dock_r.contains(last_mx,last_my);
            if over_files||was_over_files||over_dock||was_over_dock { dirty=true; }
        }
        if drag>=0 { dirty=true; } // arrastar janela recompoe
        if bounce.iter().any(|&b|b>0){dirty=true;}
        let mut app_only=false;
        if any_commit{ app_only=true; any_commit=false; }
        // apps animados (ball) pedem recomposicao da propria superficie
        if wins.iter().any(|x|x.used&&x.app_kind==2&&x.open){ app_only=true; }

        // ---- COMPOSICAO com DAMAGE RECTANGLES ----
        if dirty{
            let secs=(uptime_ms()/1000) as u32;
            // Determina o retangulo de dano.
            // - Arrastando: dano = posicao antiga U nova da janela (so o rastro
            //   + o destino precisam ser recompostos, nao a tela toda).
            // - Caso geral (abrir/fechar/foco/menu): dano = tela inteira (seguro).
            let full=Rect{x:0,y:0,w,h};
            let mut dmg = if drag>=0 && drag_prev_r.w>0 {
                rect_union(drag_prev_r, wins[drag as usize].r)
            } else if let Some(d)=damage {
                // dano especifico de abrir/fechar; clampa a tela
                let x1=(d.x+d.w).min(w); let y1=(d.y+d.h).min(h);
                Rect{x:d.x.min(w),y:d.y.min(h),w:x1.saturating_sub(d.x.min(w)),h:y1.saturating_sub(d.y.min(h))}
            } else { full };
            // inclui a regiao do cursor (atual e anterior) no dano, senao o
            // present parcial pode nao cobrir onde o cursor esta/estava.
            if dmg.w<w || dmg.h<h {
                let cur=Rect{x:mouse.x.min(last_mx),y:mouse.y.min(last_my),
                             w:mouse.x.abs_diff(last_mx)+20,h:mouse.y.abs_diff(last_my)+26};
                dmg=rect_union(dmg,cur);
                // reclampa aos limites da tela
                let x1=(dmg.x+dmg.w).min(w); let y1=(dmg.y+dmg.h).min(h);
                dmg=Rect{x:dmg.x.min(w),y:dmg.y.min(h),w:x1-dmg.x.min(w),h:y1-dmg.y.min(h)};
            }
            cursor.hide(&fb);
            compose_scene(&mut fb, dmg, w, secs, &wins, zmax, menu_open,
                          &fcache, nfiles, sel, &mouse,
                          dock_x,dock_y,dock_w,icon,gap,n_apps,&apps,&mut bounce);
            cursor.show(&fb);
            if dmg.w>=w && dmg.h>=h { fb_present(); }
            else { fb_present_rect(dmg); }
            // registra a posicao da janela arrastada p/ o proximo frame
            if drag>=0 { drag_prev_r=wins[drag as usize].r; } else { drag_prev_r.w=0; }
            damage=None;
            dirty=false;
        } else if app_only {
            // SO reblita as janelas de app committadas e apresenta suas regioes.
            // Nao recompoe wallpaper/menu/dock (caro). O cursor e restaurado
            // por cima se sobrepor a alguma area de app.
            cursor.hide(&fb);
            let ord=z_asc(&wins);
            let mut minx=w; let mut miny=h; let mut maxx=0u32; let mut maxy=0u32;
            for &wi in ord.iter(){
                if wi>=MAXWIN{continue;}
                let win=wins[wi];
                if !win.used||!win.open||win.kind!=WKind::App||!win.committed{continue;}
                blit_surface(&fb,win.slot as u64,win.client());
                let c=win.client();
                if c.x<minx{minx=c.x;} if c.y<miny{miny=c.y;}
                if c.x+c.w>maxx{maxx=c.x+c.w;} if c.y+c.h>maxy{maxy=c.y+c.h;}
            }
            cursor.show(&fb);
            if maxx>minx && maxy>miny {
                fb_present_rect(Rect{x:minx,y:miny,w:(maxx-minx).min(w-minx),h:(maxy-miny).min(h-miny)});
            }
        } else {
            // so o cursor mexeu
            let ax=last_mx.min(mouse.x); let ay=last_my.min(mouse.y);
            let bx=last_mx.max(mouse.x)+20; let by=last_my.max(mouse.y)+26;
            cursor.hide(&fb); cursor.show(&fb);
            fb_present_rect(Rect{x:ax,y:ay,w:(bx-ax).min(w-ax),h:(by-ay).min(h-ay)});
        }
        last_mx=mouse.x; last_my=mouse.y;

        let target=frame_start+16;
        while uptime_ms()<target{ yield_now(); }
        frame_start=uptime_ms();
        tick=tick.wrapping_add(1);
        if tick%120==0{ let nn=fs_count_ipc(dd); if nn!=nfiles{nfiles=nn; refresh_files(dd,nfiles,&mut fcache); dirty=true;} }
        yield_now();
    }
}

#[derive(Clone,Copy)]
struct FileMeta{ name:[u8;32], nlen:u8, size:i64, kind:u32 }
fn refresh_files(dd:usize,n:u32,cache:&mut[FileMeta;9]){
    for i in 0..n.min(9){ let mut name=[0u8;32]; let (size,kind)=fs_stat_ipc(dd,i as usize,&mut name);
        let ne=name.iter().position(|&x|x==0).unwrap_or(32);
        cache[i as usize]=FileMeta{name,nlen:ne as u8,size,kind}; }
}

fn focused_app(wins:&[Win;MAXWIN],zmax:u8)->Option<usize>{
    for i in 0..MAXWIN{ if wins[i].used&&wins[i].open&&wins[i].kind==WKind::App&&wins[i].z==zmax{return Some(i);} }
    None
}
fn z_desc(wins:&[Win;MAXWIN])->[usize;MAXWIN]{
    let mut idx=[0usize;MAXWIN]; for i in 0..MAXWIN{idx[i]=i;}
    // ordena por z decrescente (bubble simples)
    for a in 0..MAXWIN{ for b in a+1..MAXWIN{ if wins[idx[b]].z>wins[idx[a]].z{idx.swap(a,b);} } }
    idx
}
fn z_asc(wins:&[Win;MAXWIN])->[usize;MAXWIN]{
    let mut idx=z_desc(wins); idx.reverse(); idx
}

fn toggle_max(win:&mut Win,w:u32,h:u32){
    if win.max{win.r=win.saved;win.max=false;}
    else{win.saved=win.r;win.r=Rect{x:8,y:MENU_H+8,w:w-16,h:h-MENU_H-DOCK_H-28};win.max=true;}
}
fn launch(wins:&mut[Win;MAXWIN],a:&AppDef,zmax:&mut u8,next_slot:&mut u8,bounce:&mut[u32;8],dock_i:usize){
    // ja aberto? traz pra frente
    for i in 0..MAXWIN{ if wins[i].used&&wins[i].app_kind==a.kind&&wins[i].open{ *zmax+=1; wins[i].z=*zmax; return; } }
    // acha slot de janela livre
    let Some(idx)=(1..MAXWIN).find(|&i|!wins[i].used) else {return};
    let pid=spawn(a.bin);
    if pid<0{return;}
    let slot=*next_slot % 6; *next_slot=next_slot.wrapping_add(1);
    *zmax+=1;
    // posicao em cascata
    let ox=(idx as u32)*30; let oy=(idx as u32)*28;
    wins[idx]=Win{used:true,kind:WKind::App,
        r:Rect{x:120+ox,y:MENU_H+50+oy,w:a.w,h:a.h+TITLE_H},
        saved:Rect{x:120+ox,y:MENU_H+50+oy,w:a.w,h:a.h+TITLE_H},
        open:true,max:false,z:*zmax,pid:-1,slot:slot as u8,app_kind:a.kind,
        committed:false,want_close:false,ev_mx:-1,ev_my:-1,ev_click:false,ev_key:0,ev_menu:0};
    if dock_i<6{bounce[dock_i]=18;}
    // foco de teclado fica no WM, que encaminha via ev_key
}
fn open_file_by_ext(wins:&mut[Win;MAXWIN],apps:&[AppDef;8],cache:&[FileMeta;9],idx:i32,zmax:&mut u8,next_slot:&mut u8,bounce:&mut[u32;8]){
    let fm=&cache[idx as usize];
    let name=&fm.name[..fm.nlen as usize];
    // Se o arquivo e um executavel .pulse cujo nome bate com um app do
    // catalogo, EXECUTA aquele app. Senao (texto/dado), abre no editor.
    if fm.kind==1 {
        for (di,a) in apps.iter().enumerate(){
            if !a.bin.is_empty() && a.bin.as_bytes()==name {
                launch(wins,a,zmax,next_slot,bounce,di);
                return;
            }
        }
        // .pulse desconhecido: nao faz nada (nao abre no editor um binario)
        return;
    }
    // texto/dado -> editor (kind 1)
    for a in apps.iter(){ if a.kind==1{ launch(wins,a,zmax,next_slot,bounce,1); return; } }
}
fn file_at(win:&Win,mx:u32,my:u32,n:u32)->i32{
    let cy=win.r.y+52;
    for i in 0..n.min(9){ let y=cy+i*30; if mx>=win.r.x+10&&mx<win.r.x+win.r.w-10&&(my+4)>=y&&my<y+24{return i as i32;} }
    -1
}

// ---- barra de menu interativa ----
// menus: [nome_app] Arquivo Editar Janela. Retorna indice do menu clicado.
// Menus por-programa. Indice = app_kind (0=Finder/Files, 1=Editor, 2=Ball,
// 3=Calc, 4=Sobre, 5=Terminal). Cada app expoe ate 3 menus alem do seu nome.
// Um label vazio (b"") significa "menu ausente".
fn menu_labels_for(app_kind:u8)->[&'static[u8];3]{
    match app_kind{
        1=>[b"Arquivo",b"Editar",b""],          // Editor
        2=>[b"Animacao",b"",b""],               // Ball
        3=>[b"Calcular",b"",b""],                // Calculadora
        5=>[b"Terminal",b"Editar",b""],         // Terminal
        6=>[b"Rede",b"",b""],                   // Monitor de Rede
        7=>[b"Vela",b"Historico",b""],          // Navegador
        _=>[b"Arquivo",b"Editar",b"Janela"],    // Finder (padrao)
    }
}
// Nome exibido do "menu do app" (primeiro item, em negrito).
fn app_menu_name(wins:&[Win;MAXWIN],zmax:u8)->&'static[u8]{
    match focused_app(wins,zmax){
        Some(i)=>title_for(wins[i].app_kind),
        None=>b"Finder",
    }
}
fn menu_x_positions(wins:&[Win;MAXWIN],zmax:u8)->[i32;4]{
    let app=app_menu_name(wins,zmax);
    let ak=focused_app(wins,zmax).map(|i|wins[i].app_kind).unwrap_or(0);
    let lab=menu_labels_for(ak);
    let x0=40i32;
    let x1=x0+Fb::text_ui_width(app)+24;
    // menus ausentes (label vazio) nao ocupam espaco
    let x2=if lab[0].is_empty(){x1}else{x1+Fb::text_ui_width(lab[0])+22};
    let x3=if lab[1].is_empty(){x2}else{x2+Fb::text_ui_width(lab[1])+22};
    [x0,x1,x2,x3]
}
fn menu_hit(mx:u32,wins:&[Win;MAXWIN],zmax:u8)->i32{
    let xs=menu_x_positions(wins,zmax);
    let ak=focused_app(wins,zmax).map(|i|wins[i].app_kind).unwrap_or(0);
    let lab=menu_labels_for(ak);
    // menu 0 = nome do app, 1..3 = menus do app (pulando ausentes)
    for i in (0..4).rev(){
        if i>=1 && lab[i-1].is_empty(){ continue; }
        if (mx as i32)>=xs[i]{ return i as i32; }
    }
    -1
}
fn dropdown_items(menu:i32,wins:&[Win;MAXWIN],zmax:u8)->[&'static[u8];4]{
    let ak=focused_app(wins,zmax).map(|i|wins[i].app_kind).unwrap_or(0);
    // menu 0 = menu do app (nome). menus 1..3 dependem do app.
    match (ak,menu){
        // ---- Editor ----
        (1,0)=>[b"Sobre o Editor",b"",b"",b""],
        (1,1)=>[b"Novo",b"Salvar",b"Fechar",b""],       // Arquivo
        (1,2)=>[b"Desfazer",b"Copiar",b"Colar",b""],    // Editar
        // ---- Ball ----
        (2,0)=>[b"Sobre o Ball",b"",b"",b""],
        (2,1)=>[b"Pausar",b"Reiniciar",b"",b""],        // Animacao
        // ---- Calculadora ----
        (3,0)=>[b"Sobre a Calc",b"",b"",b""],
        (3,1)=>[b"Limpar",b"",b"",b""],
        // ---- Vela (navegador) ----
        (7,0)=>[b"Sobre o Vela",b"",b"",b""],
        (7,1)=>[b"Recarregar",b"Limpar",b"Fechar",b""],
        (7,2)=>[b"(vazio)",b"",b"",b""],
        // ---- Rede ----
        (6,0)=>[b"Sobre a Rede",b"",b"",b""],
        (6,1)=>[b"Atualizar",b"Fechar",b"",b""],
        // ---- Terminal ----
        (5,0)=>[b"Sobre o Terminal",b"",b"",b""],
        (5,1)=>[b"Limpar",b"Fechar",b"",b""],           // Terminal
        (5,2)=>[b"Copiar",b"Colar",b"",b""],            // Editar
        // ---- Finder (padrao) ----
        (_,0)=>[b"Sobre o Pulsar",b"Preferencias",b"",b""],
        (_,1)=>[b"Novo",b"Abrir",b"Salvar",b"Fechar"],  // Arquivo
        (_,2)=>[b"Desfazer",b"Copiar",b"Colar",b""],    // Editar
        (_,3)=>[b"Minimizar",b"Maximizar",b"Fechar tudo",b""], // Janela
        _=>[b"",b"",b"",b""],
    }
}
fn dropdown_hit(menu:i32,mx:u32,my:u32,wins:&[Win;MAXWIN],zmax:u8)->i32{
    let xs=menu_x_positions(wins,zmax);
    let dx=xs[menu as usize] as u32;
    let items=dropdown_items(menu,wins,zmax);
    for (i,it) in items.iter().enumerate(){
        if it.is_empty(){continue;}
        let y=MENU_H+2+(i as u32)*26;
        if mx>=dx && mx<dx+180 && my>=y && my<y+26 { return (menu*10+i as i32) as i32; }
    }
    -1
}
// Codigos de acao-de-app enviados ao app em foco via ev_menu.
// O app interpreta como quiser (protocolo simples e estavel).
pub const APP_MENU_SAVE:u16=1;
pub const APP_MENU_NEW:u16=2;
pub const APP_MENU_COPY:u16=3;
pub const APP_MENU_PASTE:u16=4;
pub const APP_MENU_UNDO:u16=5;
pub const APP_MENU_CLEAR:u16=6;
pub const APP_MENU_PAUSE:u16=7;
pub const APP_MENU_RESET:u16=8;

fn send_app_menu(wins:&mut[Win;MAXWIN],zmax:u8,code:u16){
    if let Some(i)=focused_app(wins,zmax){ wins[i].ev_menu=code; }
}

fn handle_menu_action(act:i32,wins:&mut[Win;MAXWIN],zmax:u8){
    let ak=focused_app(wins,zmax).map(|i|wins[i].app_kind).unwrap_or(0);
    // act = menu*10 + item. menu 0 = menu do app.
    match (ak,act){
        // ---- Editor (ak=1) ----
        (1, 0)=>{}                                    // Sobre o Editor (no-op p/ agora)
        (1,10)=>send_app_menu(wins,zmax,APP_MENU_NEW),   // Arquivo>Novo
        (1,11)=>send_app_menu(wins,zmax,APP_MENU_SAVE),  // Arquivo>Salvar
        (1,12)=>{ if let Some(i)=focused_app(wins,zmax){wins[i].want_close=true;} } // Arquivo>Fechar
        (1,20)=>send_app_menu(wins,zmax,APP_MENU_UNDO),  // Editar>Desfazer
        (1,21)=>send_app_menu(wins,zmax,APP_MENU_COPY),
        (1,22)=>send_app_menu(wins,zmax,APP_MENU_PASTE),
        // ---- Ball (ak=2) ----
        (2,10)=>send_app_menu(wins,zmax,APP_MENU_PAUSE), // Animacao>Pausar
        (2,11)=>send_app_menu(wins,zmax,APP_MENU_RESET), // Animacao>Reiniciar
        // ---- Calculadora (ak=3) ----
        (3,10)=>send_app_menu(wins,zmax,APP_MENU_CLEAR), // Calcular>Limpar
        // ---- Vela (ak=7) ----
        (7,11)=>send_app_menu(wins,zmax,APP_MENU_CLEAR), // Vela>Limpar
        (7,12)=>{ if let Some(i)=focused_app(wins,zmax){wins[i].want_close=true;} } // Vela>Fechar
        // ---- Rede (ak=6) ----
        (6,11)=>{ if let Some(i)=focused_app(wins,zmax){wins[i].want_close=true;} } // Rede>Fechar
        // ---- Terminal (ak=5) ----
        (5,10)=>send_app_menu(wins,zmax,APP_MENU_CLEAR), // Terminal>Limpar
        (5,11)=>{ if let Some(i)=focused_app(wins,zmax){wins[i].want_close=true;} } // Terminal>Fechar
        (5,20)=>send_app_menu(wins,zmax,APP_MENU_COPY),
        (5,21)=>send_app_menu(wins,zmax,APP_MENU_PASTE),
        // ---- Finder (padrao) ----
        (_,30)=>{ if let Some(i)=focused_app(wins,zmax){ wins[i].open=false; } } // Janela>Minimizar
        (_,32)=>{ for i in 1..MAXWIN{ if wins[i].used&&wins[i].kind==WKind::App{ wins[i].want_close=true; } } } // Fechar tudo
        (_,13)=>{ if let Some(i)=focused_app(wins,zmax){ wins[i].want_close=true; } } // Arquivo>Fechar
        _=>{}
    }
}

fn surf_ptr(slot:u64)->*const u32{ (0x4600_0000 + 12*1024*1024 + slot*2*1024*1024) as *const u32 }
/// Uniao de dois retangulos (menor retangulo que contem ambos).
fn rect_union(a:Rect,b:Rect)->Rect{
    let x0=a.x.min(b.x); let y0=a.y.min(b.y);
    let x1=(a.x+a.w).max(b.x+b.w); let y1=(a.y+a.h).max(b.y+b.h);
    Rect{x:x0,y:y0,w:x1-x0,h:y1-y0}
}

/// Compoe a cena inteira restrita ao retangulo de dano `dmg` (scissor).
/// As primitivas so escrevem dentro do clip, entao recompor "tudo" custa
/// proporcional a area do dano, nao a tela inteira. Isto e o coracao do
/// damage-tracking: desenhar so o que mudou.
#[allow(clippy::too_many_arguments)]
fn compose_scene(fb:&mut Fb, dmg:Rect, w:u32, secs:u32,
                 wins:&[Win;MAXWIN], zmax:u8, menu_open:i32,
                 fcache:&[FileMeta;9], nfiles:u32, sel:i32, mouse:&Mouse,
                 dock_x:u32,dock_y:u32,dock_w:u32,icon:u32,gap:u32,n_apps:u32,
                 apps:&[AppDef;8], bounce:&mut[u32;8]){
    fb.set_clip(dmg);
    // 1) fundo (restaura wallpaper so na area do dano)
    fb.restore_wall(dmg);
    // 2) barra de menu (se o dano toca o topo)
    if dmg.y < MENU_H { draw_menubar(fb,w,secs,wins,zmax,menu_open); }
    // 3) janelas em ordem de z (tras->frente); so as que intersectam o dano
    let ord=z_asc(wins);
    for &wi in ord.iter(){
        if wi>=MAXWIN{continue;}
        let win=wins[wi];
        if !win.used||!win.open{continue;}
        if !rects_overlap(win.r,dmg){continue;}
        let focused=win.z==zmax;
        match win.kind{
            WKind::Files=>draw_files(fb,&win,fcache,nfiles,focused,sel,mouse),
            WKind::App=>{
                draw_chrome(fb,win.r,title_for(win.app_kind),focused);
                if win.committed{ blit_surface(fb,win.slot as u64,win.client()); }
                else{ let c=win.client(); fb.round_rect(Rect{x:c.x,y:c.y,w:c.w,h:c.h},0,0x00FCFDFE); }
            }
        }
    }
    // 4) dropdown (se aberto)
    if menu_open>=0{ draw_dropdown(fb,menu_open,wins,zmax); }
    // 5) dock (se o dano toca a area do dock)
    let dock_r=Rect{x:dock_x,y:dock_y,w:dock_w,h:DOCK_H};
    if rects_overlap(dock_r,dmg){
        draw_dock(fb,dock_x,dock_y,dock_w,icon,gap,n_apps,apps,bounce,wins,mouse);
    }
    fb.clear_clip();
}

/// Dois retangulos se sobrepoem?
fn rects_overlap(a:Rect,b:Rect)->bool{
    a.x < b.x+b.w && b.x < a.x+a.w && a.y < b.y+b.h && b.y < a.y+a.h
}

fn blit_surface(fb:&Fb, slot:u64, dst:Rect){
    let surf=surf_ptr(slot);
    let cw=(dst.w.min(SURF_W).min(fb.w.saturating_sub(dst.x))) as usize;
    let ch=dst.h.min(SURF_H);
    let base=fb.base_ptr();
    for y in 0..ch{
        let dy=dst.y+y;
        if dy>=fb.h {break;}
        let so=(y*SURF_W) as usize;
        let doo=(dy*fb.w+dst.x) as usize;
        unsafe{ core::ptr::copy_nonoverlapping(surf.add(so), base.add(doo), cw); }
    }
}

fn compose_wallpaper(fb:&Fb,w:u32,h:u32){
    let wf=fb.wall_fb();
    // gradiente base vertical (mais escuro no topo, azul profundo embaixo)
    wf.vgrad(Rect{x:0,y:0,w,h},WALL_TOP,WALL_BOT);
    // aurora principal (canto superior, luz fria)
    let gx=(w as i32*40/100) as i32; let gy=(h as i32*28/100) as i32;
    // segunda luz (canto inferior direito, tom mais quente/roxo)
    let gx2=(w as i32*82/100) as i32; let gy2=(h as i32*78/100) as i32;
    for y in 0..h{for x in 0..w{
        let (xi,yi)=(x as i32,y as i32);
        // aurora 1
        let dx=xi-gx; let dy=yi-gy; let dd=(dx*dx+dy*dy)as u32;
        if dd<520*520{let a=(58u32).saturating_sub(isqrt(dd)/10); if a>0{wf.blend(x,y,AURORA,a);}}
        // aurora 2 (roxo suave)
        let ex=xi-gx2; let ey=yi-gy2; let ee=(ex*ex+ey*ey)as u32;
        if ee<460*460{let a=(34u32).saturating_sub(isqrt(ee)/12); if a>0{wf.blend(x,y,0x007A6ACE,a);}}
        // vinheta suave nas bordas (escurece cantos p/ profundidade)
        let mx=(xi-(w as i32/2)).abs(); let my=(yi-(h as i32/2)).abs();
        let edge=((mx*mx+my*my) as u32).saturating_sub(360*360);
        if edge>0 { let v=(isqrt(edge)/26).min(40); if v>0 { wf.blend(x,y,0x00000000,v); } }
    }}
}
fn draw_menubar(fb:&Fb,w:u32,_secs:u32,wins:&[Win;MAXWIN],zmax:u8,menu_open:i32){
    fb.vgrad_alpha(Rect{x:0,y:0,w,h:MENU_H},0x001A2439,0x00121A2C,235);
    fb.pulsar_logo(20,MENU_H/2,9,WHITE,false);
    let app=app_menu_name(wins,zmax);
    let ak=focused_app(wins,zmax).map(|i|wins[i].app_kind).unwrap_or(0);
    let lab=menu_labels_for(ak);
    let xs=menu_x_positions(wins,zmax);
    // realca o menu aberto; nome do app sempre em destaque
    let hl=|i:usize|->u32{ if menu_open==i as i32 {WHITE} else if i==0 {WHITE} else {0x00C8D4E8} };
    fb.text_ui(xs[0],6,app,hl(0));
    // menus do app (pulando ausentes)
    if !lab[0].is_empty(){ fb.text_ui_a(xs[1],6,lab[0],hl(1),215); }
    if !lab[1].is_empty(){ fb.text_ui_a(xs[2],6,lab[1],hl(2),215); }
    if !lab[2].is_empty(){ fb.text_ui_a(xs[3],6,lab[2],hl(3),215); }
    // canto direito: indicador discreto do Pulsar (sem uptime)
    let brand=b"Pulsar OS";
    let bw=Fb::text_ui_width(brand);
    fb.text_ui_a(w as i32-bw-16,6,brand,0x0090A0C0,150);
}
fn draw_dropdown(fb:&Fb,menu:i32,wins:&[Win;MAXWIN],zmax:u8){
    let xs=menu_x_positions(wins,zmax);
    let dx=xs[menu as usize] as u32;
    let items=dropdown_items(menu,wins,zmax);
    let cnt=items.iter().filter(|x|!x.is_empty()).count() as u32;
    let boxr=Rect{x:dx-6,y:MENU_H,w:190,h:cnt*26+8};
    fb.drop_shadow(boxr,10,8);
    fb.round_rect(boxr,10,0x00FDFEFF);
    fb.round_frame(boxr,10,0x00D0D8E4,180);
    let mut yy=MENU_H+4;
    for it in items.iter(){
        if it.is_empty(){continue;}
        fb.text_ui(dx as i32+8,yy as i32,it,INK);
        yy+=26;
    }
}

fn draw_chrome(fb:&Fb,r:Rect,title:&[u8],focused:bool){
    fb.drop_shadow(r,18,if focused{20}else{10});
    fb.round_rect_grad(r,16,GLASS,GLASS_BOT);
    // barra de titulo com leve gradiente
    fb.round_rect_grad(Rect{x:r.x,y:r.y,w:r.w,h:TITLE_H},16,
        if focused{0x00FFFFFF}else{0x00F4F6FA}, if focused{0x00F6F8FC}else{0x00EDEFF4});
    // linha separadora sutil sob a barra de titulo (macOS-like)
    fb.fill(Rect{x:r.x+1,y:r.y+TITLE_H-1,w:r.w-2,h:1}, if focused{0x00E2E6EE}else{0x00E8EAF0});
    // semaforos: contorno leve quando focado, cinza quando nao
    fb.disc(r.x+24,r.y+20,7,if focused{0x00FF5F57}else{0x00D0D4DB});
    fb.disc(r.x+48,r.y+20,7,if focused{0x00FEBC2E}else{0x00D0D4DB});
    fb.disc(r.x+72,r.y+20,7,if focused{0x0028C840}else{0x00D0D4DB});
    if focused {
        // brilho sutil no topo de cada semaforo
        fb.disc(r.x+24,r.y+18,2,0x00FF9089);
        fb.disc(r.x+48,r.y+18,2,0x00FFD46B);
        fb.disc(r.x+72,r.y+18,2,0x0060DE7C);
    }
    let tw=Fb::text_ui_width(title);
    fb.text_ui(r.x as i32+r.w as i32/2-tw/2,r.y as i32+10,title,if focused{INK}else{INK_DIM});
    fb.round_frame(r,16,WHITE,if focused{70}else{35});
}
fn draw_files(fb:&Fb,win:&Win,cache:&[FileMeta;9],nfiles:u32,focused:bool,sel:i32,mouse:&Mouse){
    draw_chrome(fb,win.r,b"Arquivos",focused);
    let cx=(win.r.x+20)as i32; let cy=(win.r.y+52)as i32;
    for i in 0..nfiles.min(9){
        let fm=&cache[i as usize];
        let ne=fm.nlen as usize; let name=fm.name; let size=fm.size; let kind=fm.kind;
        let y=cy+(i as i32)*30;
        let rr=Rect{x:win.r.x+10,y:y as u32-4,w:win.r.w-20,h:28};
        if sel==i as i32{fb.round_rect(rr,6,0x005B9BF0);}
        else if rr.contains(mouse.x,mouse.y){fb.round_rect(rr,6,0x00E8EEFA);}
        let tc=if sel==i as i32{WHITE}else{INK};
        let ic=if kind==1{0x0032D256}else{0x00579BFF};
        fb.round_rect(Rect{x:cx as u32,y:(y+3)as u32,w:18,h:18},4,if sel==i as i32{WHITE}else{ic});
        fb.text_ui(cx+28,y,&name[..ne],tc);
        if size>=0{let mut sb=[0u8;10];let si=u32_dec(size as u32,&mut sb);
            let sw=Fb::text_ui_width(&sb[si..])+18;
            fb.text_ui_a((win.r.x+win.r.w)as i32-sw,y,&sb[si..],if sel==i as i32{WHITE}else{INK_DIM},220);}
    }
    fb.text_ui_a(cx,(win.r.y+win.r.h-24)as i32,b"Duplo-clique abre no editor",INK_DIM,160);
}
fn draw_dock(fb:&Fb,dx:u32,dy:u32,dw:u32,icon:u32,gap:u32,n:u32,apps:&[AppDef;8],bounce:&mut[u32;8],wins:&[Win;MAXWIN],mouse:&Mouse){
    let dock=Rect{x:dx,y:dy,w:dw,h:DOCK_H};
    fb.drop_shadow(dock,24,9);
    fb.round_rect_grad(dock,24,0x99F2F5FC,0x77DEE6F4);
    fb.round_frame(dock,24,WHITE,70);
    for i in 0..n{
        let b=&mut bounce[i as usize]; let lift=if *b>0{*b/2}else{0}; if *b>0{*b-=1;}
        let ix=dx+gap+i*(icon+gap); let iy=dy+12-lift;
        let a=&apps[i as usize];
        let hover=Rect{x:ix,y:dy+12,w:icon,h:icon}.contains(mouse.x,mouse.y);
        fb.drop_shadow(Rect{x:ix,y:iy,w:icon,h:icon},14,4);
        let top=Fb::lerp(a.color,WHITE,if hover{120}else{75});
        fb.round_rect_grad(Rect{x:ix,y:iy,w:icon,h:icon},14,top,a.color);
        fb.round_frame(Rect{x:ix,y:iy,w:icon,h:icon},14,WHITE,55);
        let g=[a.glyph]; let gw=Fb::text_ui_width(&g);
        fb.text_ui(ix as i32+(icon as i32-gw)/2,iy as i32+icon as i32/2-9,&g,WHITE);
        let running=if a.bin.is_empty(){wins[0].open}else{wins.iter().any(|x|x.used&&x.app_kind==a.kind&&x.open)};
        if running{fb.disc(ix+icon/2,dy+DOCK_H-6,2,WHITE);}
    }
}
