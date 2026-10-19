/* terminal.c — Terminal do Pulsar OS (C). Um shell com comandos no estilo
 * Unix (ls, cat, ps, kill, open, date, free, uname, neofetch...), historico
 * com as setas e completar com Tab. Desenha na propria superficie e segue o
 * tema (claro/escuro) do sistema.
 *
 * O que nao e comando interno roda como programa de /bin (binarios Linux
 * ligados a musl, ou .pulse): o stdout/stderr do filho chega por uma pipe e
 * aparece aqui; Ctrl+C encerra o programa. */
#include "pulsar.h"
#include <string.h>

#define COLS 160
#define LINES 300
#define MAXLINE 120
#define HIST 16

typedef struct { char t[COLS]; u8 kind; } Line;   /* kind: 0 saida, 1 eco, 2 erro, 3 destaque */

static Line lines[LINES];
static int nlines = 0;
static int scroll = 0;            /* linhas acima do fim */
static char cmd[MAXLINE];
static int cmdlen = 0, cur = 0;   /* bytes; cursor em bytes */
static char hist[HIST][MAXLINE];
static int nhist = 0, hpos = -1;
static App app;

/* programa externo em execucao */
static i64 child_pid = -1;
static i32 child_fd = -1;        /* ponta de leitura da pipe do stdout/stderr */
static char partial[COLS];       /* linha incompleta recebida do filho */
static int plen = 0;

static void out(const char* s, int kind){
    if(nlines == LINES){ memmove(lines, lines + 1, sizeof(Line) * (LINES - 1)); nlines--; }
    int n = 0;
    while(s[n] && n < COLS - 1){ lines[nlines].t[n] = s[n]; n++; }
    lines[nlines].t[n] = 0;
    lines[nlines].kind = (u8)kind;
    nlines++;
}

static int word(const char* s, const char* w){
    int n = strlen(w);
    return strncmp(s, w, n) == 0 && (s[n] == 0 || s[n] == ' ');
}

static const char* arg1(const char* s){
    while(*s && *s != ' ') s++;
    while(*s == ' ') s++;
    return s;
}

static int find_file(const char* name){
    u8 st[44];
    for(u64 i = 0; i < sys_fs_count(); i++){
        if(sys_fs_stat(i, st) >= 0 && strcmp((char*)st, name) == 0) return (int)i;
    }
    return -1;
}

/* ls <dir>: lista um diretorio do ext2 (getdents) */
static void cmd_ls_dir(const char* path){
    i64 fd = sys_open(path, P_O_DIRECTORY, 0);
    if(fd < 0){ out("ls: diretorio nao encontrado", 2); return; }
    static u8 buf[2048]; char b[COLS]; i64 n; int count = 0;
    while((n = sys_getdents(fd, buf, sizeof buf)) > 0){
        for(i64 o = 0; o < n; ){
            PDirent* d = (PDirent*)(buf + o); o += d->reclen;
            if(d->name[0] == '.' && (d->name[1] == 0 || (d->name[1] == '.' && d->name[2] == 0))) continue;
            char full[256]; char* q = fmt_s(full, path);
            if(q[-1] != '/') *q++ = '/';
            fmt_s(q, d->name);
            PStat st; char sz[24] = "";
            if(d->type == 8 && sys_stat(full, &st) == 0) fmt_size(sz, (u64)st.size);
            char* p = fmt_s(b, d->type == 4 ? "  d  " : d->type == 10 ? "  l  " : "  -  ");
            int pad = 10 - (int)strlen(sz); while(pad-- > 0) *p++ = ' ';
            p = fmt_s(p, sz); p = fmt_s(p, "   "); p = fmt_s(p, d->name);
            if(d->type == 4) fmt_s(p, "/");
            out(b, d->type == 4 ? 3 : 0);
            count++;
        }
    }
    sys_close(fd);
    char* p = fmt_s(b, "  "); p = fmt_u(p, (u64)count); fmt_s(p, " entradas");
    out(b, 0);
}

static void cmd_ls(void){
    u8 st[44]; char b[COLS];
    u64 n = sys_fs_count(), total = 0;
    for(u64 i = 0; i < n; i++){
        if(sys_fs_stat(i, st) < 0) continue;
        u32 size = *(u32*)(st + 36), kind = *(u32*)(st + 40);
        total += size;
        char* p = b;
        p = fmt_s(p, kind == 1 ? "  -rwx  " : "  -rw-  ");
        char sz[24]; fmt_size(sz, size);
        int pad = 10 - (int)strlen(sz);
        while(pad-- > 0) *p++ = ' ';
        p = fmt_s(p, sz); p = fmt_s(p, "   ");
        fmt_s(p, (char*)st);
        out(b, kind == 1 ? 3 : 0);
    }
    char* p = fmt_s(b, "  "); p = fmt_u(p, n); p = fmt_s(p, " arquivos, "); fmt_size(p, total);
    out(b, 0);
}

static void cmd_cat(const char* name){
    static char buf[16384];
    int idx = find_file(name);
    if(idx < 0){ out("cat: arquivo nao encontrado", 2); return; }
    i64 n = sys_fs_read((u64)idx, buf, sizeof buf - 1);
    if(n < 0){ out("cat: erro de leitura", 2); return; }
    buf[n] = 0;
    char line[COLS]; int k = 0;
    for(i64 i = 0; i <= n; i++){
        char c = buf[i];
        if(c == '\n' || c == 0 || k == COLS - 2){
            line[k] = 0; out(line, 0); k = 0;
            if(c == 0) break;
            if(c != '\n') line[k++] = c;
        } else if((u8)c >= 32 || (u8)c >= 0x80) line[k++] = c;
        else if(c == '\t'){ for(int t = 0; t < 4 && k < COLS - 2; t++) line[k++] = ' '; }
    }
}

static void cmd_ps(void){
    ProcInfo p; char b[COLS];
    static const char* st[] = {"?", "rodando", "pronto", "dormindo", "esperando"};
    out("  PID  CPU  ESTADO      TEMPO      NOME", 3);
    for(u64 s = 0;; s++){
        i64 r = sys_proc_info(s, &p);
        if(r == -2) break;
        if(r < 0) continue;
        char* q = b; char t[24];
        fmt_u(t, p.pid); int pad = 5 - (int)strlen(t); while(pad-- > 0) *q++ = ' ';
        q = fmt_s(q, t); q = fmt_s(q, "  ");
        q = fmt_u(q, p.cpu); q = fmt_s(q, "    ");
        const char* sn = p.state < 5 ? st[p.state] : "?";
        q = fmt_s(q, sn); pad = 12 - (int)strlen(sn); while(pad-- > 0) *q++ = ' ';
        fmt_u(t, p.cpu_ms); q = fmt_s(q, t); q = fmt_s(q, " ms");
        pad = 8 - (int)strlen(t); while(pad-- > 0) *q++ = ' ';
        fmt_s(q, p.name);
        out(b, 0);
    }
}

static void cmd_date(void){
    static const char* wd[] = {"domingo", "segunda", "terca", "quarta", "quinta", "sexta", "sabado"};
    static const char* mo[] = {"jan","fev","mar","abr","mai","jun","jul","ago","set","out","nov","dez"};
    u32 y, m, d, h, mi, s, w;
    civil_time(sys_rtc(), &y, &m, &d, &h, &mi, &s, &w);
    char b[64], *p = b;
    p = fmt_s(p, wd[w % 7]); p = fmt_s(p, ", "); p = fmt_u(p, d); p = fmt_s(p, " de ");
    p = fmt_s(p, mo[(m + 11) % 12]); p = fmt_s(p, " de "); p = fmt_u(p, y); p = fmt_s(p, "  ");
    if(h < 10) *p++ = '0'; p = fmt_u(p, h); *p++ = ':';
    if(mi < 10) *p++ = '0'; p = fmt_u(p, mi); *p++ = ':';
    if(s < 10) *p++ = '0'; p = fmt_u(p, s); fmt_s(p, " UTC");
    out(b, 0);
}

static void cmd_free(void){
    SysInfo si; sys_sysinfo(&si);
    char b[COLS], *p = b;
    out("         total        usado        livre", 3);
    char t[24]; p = fmt_s(b, "Mem:  ");
    fmt_size(t, (u64)si.mem_total_kb * 1024); p = fmt_s(p, t); p = fmt_s(p, "     ");
    fmt_size(t, (u64)(si.mem_total_kb - si.mem_free_kb) * 1024); p = fmt_s(p, t); p = fmt_s(p, "     ");
    fmt_size(t, (u64)si.mem_free_kb * 1024); fmt_s(p, t);
    out(b, 0);
}

static void cmd_cpus(void){
    char b[COLS];
    for(u64 c = 0; c < 8; c++){
        i64 l = sys_cpu_load(c);
        if(l < 0) continue;
        char* p = fmt_s(b, "cpu"); p = fmt_u(p, c); p = fmt_s(p, "  [");
        for(int k = 0; k < 20; k++) *p++ = k < l / 5 ? '#' : '.';
        p = fmt_s(p, "]  "); p = fmt_u(p, (u64)l); fmt_s(p, "%");
        out(b, 0);
    }
}

static void cmd_uname(void){
    SysInfo si; sys_sysinfo(&si);
    char b[COLS], *p = fmt_s(b, "Pulsar OS 0.2 (Nebulosa) ");
    p = fmt_s(p, si.arch_name); p = fmt_s(p, " SMP "); p = fmt_u(p, si.ncpus); fmt_s(p, " CPU(s) microkernel");
    out(b, 0);
}

static void cmd_neofetch(void){
    SysInfo si; sys_sysinfo(&si);
    static const char* logo[] = {
        "     .-~~~-.     ", "   .'  .-.  '.   ", "  /   (   )   \\  ", " |  ~~/\\_/\\~~  | ",
        "  \\   (   )   /  ", "   '.  '-'  .'   ", "     '-~~~-'     ",
    };
    char info[7][COLS]; char t[32];
    fmt_s(info[0], "usuario@pulsar");
    fmt_s(info[1], "--------------");
    char* p = fmt_s(info[2], "SO: Pulsar OS 0.2 "); fmt_s(p, si.arch_name);
    p = fmt_s(info[3], "Kernel: microkernel SMP, "); p = fmt_u(p, si.ncpus); fmt_s(p, " CPU(s)");
    p = fmt_s(info[4], "Resolucao: "); p = fmt_u(p, si.fb_w); p = fmt_s(p, "x"); fmt_u(p, si.fb_h);
    fmt_size(t, (u64)(si.mem_total_kb - si.mem_free_kb) * 1024);
    p = fmt_s(info[5], "Memoria: "); p = fmt_s(p, t); p = fmt_s(p, " / "); fmt_size(t, (u64)si.mem_total_kb * 1024); fmt_s(p, t);
    u64 up = si.uptime_ms / 1000;
    p = fmt_s(info[6], "Ativo ha: "); p = fmt_u(p, up / 60); p = fmt_s(p, " min "); p = fmt_u(p, up % 60); fmt_s(p, " s");
    for(int i = 0; i < 7; i++){
        char b[COLS]; char* q = fmt_s(b, logo[i]); q = fmt_s(q, "  "); fmt_s(q, info[i]);
        out(b, i == 0 ? 3 : 0);
    }
}

static const char* CMDS[] = {"help", "clear", "echo", "ls", "cat", "ps", "kill", "open", "date",
    "uptime", "free", "cpus", "uname", "neofetch", "history", "exit", "poweroff", 0};

/* ---- programas externos ---- */

/* texto vindo do filho: quebra em linhas (tabs viram espacos) */
static void feed(const char* s, i64 n){
    for(i64 i = 0; i < n; i++){
        char c = s[i];
        if(c == '\n' || plen == COLS - 2){
            partial[plen] = 0; out(partial, 0); plen = 0;
            if(c == '\n') continue;
        }
        if(c == '\t'){ do partial[plen++] = ' '; while(plen % 4 && plen < COLS - 2); }
        else if((u8)c >= 32 || c == 0x1B) partial[plen++] = c;
    }
}

/* le o que o filho escreveu; quando a pipe fecha, colhe o codigo de saida */
static int pump(void){
    int got = 0;
    if(child_fd >= 0){
        static char buf[4096];
        for(int k = 0; k < 16; k++){
            i64 n = sys_read(child_fd, buf, sizeof buf);
            if(n > 0){ feed(buf, n); got = 1; continue; }
            if(n == 0){                                   /* fim: o filho fechou o stdout */
                if(plen){ partial[plen] = 0; out(partial, 0); plen = 0; }
                sys_close(child_fd); child_fd = -1; got = 1;
            }
            break;                                        /* -EAGAIN: nada por enquanto */
        }
    }
    if(child_fd < 0 && child_pid >= 0){
        i32 st = 0;
        i64 r = sys_wait(child_pid, &st, 1);
        if(r != 0){
            if(r > 0 && (st & 0x7F)){ char b[64]; char* p = fmt_s(b, "[encerrado pelo sinal "); p = fmt_u(p, (u64)(st & 0x7F)); fmt_s(p, "]"); out(b, 2); }
            else if(r > 0 && (st >> 8)){ char b[64]; char* p = fmt_s(b, "[saiu com codigo "); p = fmt_u(p, (u64)((st >> 8) & 0xFF)); fmt_s(p, "]"); out(b, 2); }
            child_pid = -1; got = 1;
        }
    }
    return got;
}

/* "prog a b" -> executa /bin/prog (ou o caminho dado) com stdout numa pipe */
static int run_external(const char* c){
    char argv[MAXLINE * 2]; int alen = 0, argc = 0;
    char prog[MAXLINE] = {0};
    while(*c){
        while(*c == ' ') c++;
        if(!*c) break;
        char q = 0; if(*c == '"' || *c == '\''){ q = *c++; }
        int start = alen;
        while(*c && (q ? *c != q : *c != ' ')) argv[alen++] = *c++;
        if(q && *c == q) c++;
        argv[alen++] = 0;
        if(argc == 0) memcpy(prog, argv + start, alen - start);
        argc++;
    }
    if(argc == 0) return 1;
    char path[MAXLINE + 8];
    if(strchr(prog, '/')) fmt_s(path, prog); else { char* p = fmt_s(path, "/bin/"); fmt_s(p, prog); }
    PStat st;
    if(sys_stat(path, &st) != 0 || (st.mode & 0170000) != 0100000) return 0;
    static const char* env[] = {"HOME=/home/user", "PATH=/bin:/usr/bin", "TERM=dumb", "USER=usuario", "LANG=C.UTF-8",
                                "XDG_RUNTIME_DIR=/run", "WAYLAND_DISPLAY=wayland-0",
                                "XCOMPOSEFILE=/usr/share/pulsar/Compose",
                                "GDK_BACKEND=wayland", "MOZ_ENABLE_WAYLAND=1", "NO_AT_BRIDGE=1",
                                "GSETTINGS_BACKEND=memory", "MOZ_CRASHREPORTER_DISABLE=1",
                                "MOZ_DISABLE_CONTENT_SANDBOX=1", "MOZ_DISABLE_GMP_SANDBOX=1",
                                "MOZ_DISABLE_RDD_SANDBOX=1", "MOZ_DISABLE_SOCKET_PROCESS_SANDBOX=1",
                                "LIBGL_ALWAYS_SOFTWARE=1", 0};
    static u8 block[2048];
    int envc = 0, n = 8;
    memcpy(block + n, argv, alen); n += alen;
    for(; env[envc]; envc++){ int l = strlen(env[envc]) + 1; memcpy(block + n, env[envc], l); n += l; }
    *(u32*)block = (u32)argc; *(u32*)(block + 4) = (u32)envc;
    /* stdout/stderr do filho = ponta de escrita de uma pipe */
    i32 p[2];
    if(sys_pipe(p, P_O_CLOEXEC) < 0){ out("erro: sem pipes livres", 2); return 1; }
    sys_fcntl(p[0], 4, P_O_NONBLOCK);                     /* F_SETFL: o terminal nao bloqueia */
    i64 save = sys_dup(1, -1); sys_fcntl(save, 2, 1);     /* guarda o console (close-on-exec) */
    sys_dup(p[1], 1); sys_dup(p[1], 2);
    i64 pid = sys_spawn_ex(path, block, (u64)n);
    sys_dup(save, 1); sys_dup(save, 2); sys_close(save); sys_close(p[1]);
    if(pid < 0){ sys_close(p[0]); out("erro: nao foi possivel executar", 2); return 1; }
    child_pid = pid; child_fd = p[0]; plen = 0;
    return 1;
}

static void run(void){
    char echo[COLS]; char* p = fmt_s(echo, "pulsar ~ $ "); fmt_s(p, cmd);
    out(echo, 1);
    const char* c = cmd;
    while(*c == ' ') c++;
    if(!*c) return;
    if(nhist == HIST){ memmove(hist, hist + 1, sizeof(hist[0]) * (HIST - 1)); nhist--; }
    strcpy(hist[nhist++], c);
    if(word(c, "help")){
        out("Comandos disponiveis:", 3);
        out("  ls  cat <arq>  ps  kill <pid>  open <app>  date  uptime", 0);
        out("  free  cpus  uname  neofetch  echo <txt>  history  clear  exit  poweroff", 0);
        out("Apps: editor, calc, vela, netmon, sysmon, ball (ex.: open calc)", 0);
        out("Programas de /bin rodam pelo nome (ex.: posixtest, spawntest); ls /bin", 0);
        out("Setas: historico | Tab: completar | Ctrl+L: limpar", 0);
    } else if(word(c, "clear")){ nlines = 0; }
    else if(word(c, "echo")){ out(arg1(c), 0); }
    else if(word(c, "ls")){ if(arg1(c)[0] == '/') cmd_ls_dir(arg1(c)); else cmd_ls(); }
    else if(word(c, "cat")){ cmd_cat(arg1(c)); }
    else if(word(c, "ps")){ cmd_ps(); }
    else if(word(c, "kill")){
        const char* a = arg1(c); u64 pid = 0;
        if(!*a){ out("uso: kill <pid>", 2); return; }
        while(*a >= '0' && *a <= '9') pid = pid * 10 + (u64)(*a++ - '0');
        i64 r = sys_kill(pid);
        out(r == 0 ? "processo encerrado" : "kill: pid invalido", r == 0 ? 0 : 2);
    } else if(word(c, "open")){
        const char* a = arg1(c); char name[40];
        char* q = fmt_s(name, a);
        if(strlen(a) < 6 || strcmp(q - 6, ".pulse") != 0) fmt_s(q, ".pulse");
        i64 pid = sys_spawn(name, strlen(name));
        char b[64]; if(pid >= 0){ char* r = fmt_s(b, "iniciado: pid "); fmt_u(r, (u64)pid); out(b, 0); }
        else out("open: programa nao encontrado", 2);
    } else if(word(c, "date")){ cmd_date(); }
    else if(word(c, "uptime")){
        u64 s = sys_time_ms() / 1000; char b[64];
        char* r = fmt_s(b, "ativo ha "); r = fmt_u(r, s / 3600); r = fmt_s(r, "h "); r = fmt_u(r, (s / 60) % 60);
        r = fmt_s(r, "min "); r = fmt_u(r, s % 60); fmt_s(r, "s"); out(b, 0);
    }
    else if(word(c, "free")){ cmd_free(); }
    else if(word(c, "cpus")){ cmd_cpus(); }
    else if(word(c, "uname")){ cmd_uname(); }
    else if(word(c, "neofetch")){ cmd_neofetch(); }
    else if(word(c, "history")){ for(int i = 0; i < nhist; i++) out(hist[i], 0); }
    else if(word(c, "exit")){ sys_exit(); }
    else if(word(c, "poweroff")){ sys_poweroff(); }
    else if(!run_external(c)){ char b[COLS]; char* r = fmt_s(b, c); fmt_s(r, ": comando nao encontrado (digite help)"); out(b, 2); }
}

static void complete(void){
    for(int i = 0; CMDS[i]; i++){
        if(cmdlen > 0 && strncmp(CMDS[i], cmd, cmdlen) == 0){
            fmt_s(cmd, CMDS[i]); cmdlen = cur = strlen(cmd);
            cmd[cmdlen++] = ' '; cmd[cmdlen] = 0; cur = cmdlen;
            return;
        }
    }
}

static void insert(u32 ch){
    char e[4]; int n = utf8_encode(ch, e);
    if(cmdlen + n >= MAXLINE - 1) return;
    memmove(cmd + cur + n, cmd + cur, cmdlen - cur);
    memcpy(cmd + cur, e, n);
    cmdlen += n; cur += n; cmd[cmdlen] = 0;
}

static void key(WmEvent* e){
    if(child_pid >= 0){                                   /* programa rodando: so Ctrl+C */
        if((e->mods & MOD_CTRL) && e->key == 46){ sys_kill((u64)child_pid); out("^C", 1); }
        return;
    }
    switch(e->key){
    case KEY_ENTER: case KEY_KPENTER: run(); cmdlen = cur = 0; cmd[0] = 0; hpos = -1; scroll = 0; return;
    case KEY_BACKSPACE:
        if(cur > 0){
            int s = cur - 1; while(s > 0 && ((u8)cmd[s] & 0xC0) == 0x80) s--;
            memmove(cmd + s, cmd + cur, cmdlen - cur); cmdlen -= cur - s; cur = s; cmd[cmdlen] = 0;
        }
        return;
    case KEY_LEFT: while(cur > 0){ cur--; if(((u8)cmd[cur] & 0xC0) != 0x80) break; } return;
    case KEY_RIGHT: while(cur < cmdlen){ cur++; if(cur == cmdlen || ((u8)cmd[cur] & 0xC0) != 0x80) break; } return;
    case KEY_HOME: cur = 0; return;
    case KEY_END: cur = cmdlen; return;
    case KEY_TAB: complete(); return;
    case KEY_PGUP: scroll += 10; return;
    case KEY_PGDN: scroll = scroll > 10 ? scroll - 10 : 0; return;
    case KEY_UP: case KEY_DOWN:
        if(nhist == 0) return;
        if(e->key == KEY_UP) hpos = hpos < 0 ? nhist - 1 : (hpos > 0 ? hpos - 1 : 0);
        else { if(hpos < 0) return; hpos++; if(hpos >= nhist){ hpos = -1; cmd[0] = 0; cmdlen = cur = 0; return; } }
        fmt_s(cmd, hist[hpos]); cmdlen = cur = strlen(cmd);
        return;
    }
    if((e->mods & MOD_CTRL) && e->key == 38){ nlines = 0; return; }   /* Ctrl+L */
    if((e->mods & MOD_CTRL) && e->key == 46){ out("^C", 1); cmdlen = cur = 0; cmd[0] = 0; return; }
    if(e->ch >= 32) insert(e->ch);
}

static i32 prompt_y = -1;   /* linha do prompt no ultimo draw (-1: nenhuma) */

static void draw(int caret){
    Fb* fb = &app.fb;
    const Theme* th = &app.th;
    u32 bg = th->dark ? 0x1C1C1E : 0xFFFFFF;
    u32 fg = th->dark ? 0xE8E8ED : 0x1D1D1F;
    u32 dim = th->dark ? 0x8E8E93 : 0x6E6E73;
    u32 err = th->dark ? 0xFF6961 : 0xD70015;
    u32 prompt = th->dark ? 0x64D27A : 0x1E8E3E;
    fb_fill(fb, rect(0, 0, app.w, app.h), bg);
    const Font* f = &FONT_MONO;
    i32 lh = f->height + 1;
    i32 rows = (app.h - 16) / lh;
    int total = nlines + 1;
    int maxs = total > rows ? total - rows : 0;
    if(scroll > maxs) scroll = maxs;
    int first = total - rows - scroll; if(first < 0) first = 0;
    i32 y = 8;
    prompt_y = -1;
    for(int i = first; i < nlines && y < app.h - lh; i++, y += lh){
        u32 c = lines[i].kind == 2 ? err : lines[i].kind == 3 ? th->accent : lines[i].kind == 1 ? dim : fg;
        text_draw(fb, f, 10, y, lines[i].t, c);
    }
    if(scroll == 0 && child_pid < 0){
        prompt_y = y;
        i32 x = 10 + text_draw(fb, f, 10, y, "pulsar", prompt);
        x += text_draw(fb, f, x, y, " ~ $ ", th->accent);
        text_draw(fb, f, x, y, cmd, fg);
        i32 cx = x + text_width_n(f, cmd, cur);
        if(caret) fb_fill_a(fb, rect(cx, y + 2, 8, f->height - 2), fg, app.focused ? 160 : 60);
    }
}

int main(void){
    app_init(&app, 5, "Terminal", 600, 380);
    out("Pulsar OS - Terminal. Digite 'help' para ver os comandos.", 3);
    int caret = 1; u64 blink = sys_time_ms();
    int full = 1, last_n = -1, last_s = -1; i32 last_w = 0, last_h = 0;
    draw(caret);
    WmEvent e = app_commit(&app);
    for(;;){
        if(e.close) sys_exit();
        int dirty = e.changed;
        if(e.changed) full = 1;
        if(e.menu == APP_MENU_CLEAR){ nlines = 0; dirty = 1; }
        if(e.key){ key(&e); caret = 1; blink = sys_time_ms(); dirty = 1; }
        if(e.wheel){ scroll += e.wheel * 3; if(scroll < 0) scroll = 0; dirty = 1; }
        if(sys_time_ms() - blink > 530){ caret = !caret; blink = sys_time_ms(); dirty = 1; }
        if(pump()){ scroll = 0; dirty = 1; full = 1; }
        if(e.more){ e = app_poll(&app); continue; }
        if(dirty){
            /* so a linha do prompt mudou (digitacao, cursor piscando)? o WM
             * recompoe so ela em vez da janela inteira */
            int n0 = nlines, s0 = scroll; i32 p0 = prompt_y, w0 = app.w, h0 = app.h;
            draw(caret);
            i32 lh = FONT_MONO.height + 1;
            int only = !full && n0 == last_n && s0 == last_s && p0 == prompt_y && prompt_y >= 0
                       && w0 == last_w && h0 == last_h;
            last_n = nlines; last_s = scroll; last_w = app.w; last_h = app.h; full = 0;
            e = only ? app_commit_rect(&app, rect(0, prompt_y, app.w, lh)) : app_commit(&app);
        }
        else if(child_pid >= 0){ sys_sleep(16); e = app_peek(&app); }   /* acompanha a saida */
        else e = app_poll(&app);
    }
    return 0;
}
