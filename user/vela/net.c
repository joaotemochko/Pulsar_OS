/* net.c — DNS, conexoes (TCP puro ou TLS) e cliente HTTP/1.1.
 * O kernel so oferece sockets: tudo daqui para cima e do Vela. */
#include "vela.h"

/* ------------------------------------------------------------------ DNS */
#define DNS_SERVER ((10u << 24) | (0u << 16) | (2u << 8) | 3u)    /* resolvedor do SLIRP */
static struct { char name[128]; u32 ip; } dns_cache[32];
static int dns_n = 0;

static int is_ipv4(const char* h, u32* ip){
    u32 parts[4] = {0}; int k = 0, digits = 0;
    for(const char* s = h; *s; s++){
        if(*s == '.'){ if(++k > 3 || !digits) return 0; digits = 0; }
        else if(*s >= '0' && *s <= '9'){ parts[k] = parts[k] * 10 + (u32)(*s - '0'); digits++; if(parts[k] > 255) return 0; }
        else return 0;
    }
    if(k != 3 || !digits) return 0;
    *ip = (parts[0] << 24) | (parts[1] << 16) | (parts[2] << 8) | parts[3];
    return 1;
}

/* pula um nome (com ou sem compressao) na mensagem DNS */
static int dns_skip_name(const u8* a, int len, int p){
    while(p < len){
        if((a[p] & 0xC0) == 0xC0) return p + 2;
        if(a[p] == 0) return p + 1;
        p += a[p] + 1;
    }
    return len;
}

int dns_resolve(const char* name, u32* ip){
    if(is_ipv4(name, ip)) return 1;
    if(strcmp(name, "localhost") == 0){ *ip = 0x7F000001; return 1; }
    for(int i = 0; i < dns_n; i++) if(strcmp(dns_cache[i].name, name) == 0){ *ip = dns_cache[i].ip; return 1; }
    static u16 qid = 0x5150;
    /* NET_UDP_SEND leva o ponteiro em 32 bits: o buffer fica na .bss (endereco
     * baixo), nunca na pilha de uma thread (que mora no heap, acima de 4 GiB) */
    static u8 q[300]; int n = 0;
    qid++;
    q[n++] = (u8)(qid >> 8); q[n++] = (u8)qid;
    q[n++] = 0x01; q[n++] = 0x00;                       /* recursao desejada */
    q[n++] = 0; q[n++] = 1; q[n++] = 0; q[n++] = 0; q[n++] = 0; q[n++] = 0; q[n++] = 0; q[n++] = 0;
    const char* s = name;
    while(*s){
        const char* d = s; while(*d && *d != '.') d++;
        int l = (int)(d - s);
        if(l == 0 || l > 63 || n + l + 2 > 280) return 0;
        q[n++] = (u8)l; memcpy(q + n, s, (size_t)l); n += l;
        s = *d ? d + 1 : d;
    }
    q[n++] = 0;
    q[n++] = 0; q[n++] = 1; q[n++] = 0; q[n++] = 1;   /* tipo A, classe IN */
    u16 sport = (u16)(53000 + (qid & 0x3FF));
    static u8 r[1500];
    for(int tries = 0; tries < 3; tries++){
        if(!sys_net_udp_send(DNS_SERVER, 53, sport, q, (u32)n)) return 0;
        u64 t0 = sys_time_ms();
        while(sys_time_ms() - t0 < 2000){
            i64 got = sys_udp_recv(sport, r, sizeof r);
            if(got < 0){ sys_sleep(5); continue; }
            u8* a = r + 8; int len = (int)got - 8;
            if(len < 12 || ((a[0] << 8) | a[1]) != qid) continue;
            int an = (a[6] << 8) | a[7];
            int p = dns_skip_name(a, len, 12) + 4;      /* pula a pergunta */
            for(int k = 0; k < an && p + 10 <= len; k++){
                p = dns_skip_name(a, len, p);
                if(p + 10 > len) break;
                int type = (a[p] << 8) | a[p + 1];
                int rdl = (a[p + 8] << 8) | a[p + 9];
                p += 10;
                if(type == 1 && rdl == 4 && p + 4 <= len){
                    *ip = ((u32)a[p] << 24) | ((u32)a[p+1] << 16) | ((u32)a[p+2] << 8) | a[p+3];
                    int slot = dns_n < 32 ? dns_n++ : (int)(qid % 32);
                    snprintf(dns_cache[slot].name, sizeof dns_cache[slot].name, "%s", name);
                    dns_cache[slot].ip = *ip;
                    return 1;
                }
                p += rdl;
            }
            return 0;
        }
    }
    return 0;
}

/* ------------------------------------------------------------- cookies */
typedef struct Cookie { char domain[128]; char path[128]; char name[96]; char* value; int host_only; struct Cookie* next; } Cookie;
static Cookie* jar = NULL;

static int domain_match(const char* host, const Cookie* c){
    if(c->host_only) return strcmp(host, c->domain) == 0;
    int hl = (int)strlen(host), dl = (int)strlen(c->domain);
    if(hl < dl) return 0;
    if(strcmp(host + hl - dl, c->domain) != 0) return 0;
    return hl == dl || host[hl - dl - 1] == '.';
}

static Mutex jar_lock = MUTEX_INIT;     /* cookies: rede e JS (document.cookie) */

void cookies_get(const char* host, const char* path, Buf* out){
    mutex_lock(&jar_lock);
    for(Cookie* c = jar; c; c = c->next){
        if(!domain_match(host, c)) continue;
        if(strncmp(path, c->path, strlen(c->path)) != 0) continue;
        if(out->n) buf_puts(out, "; ");
        buf_printf(out, "%s=%s", c->name, c->value);
    }
    mutex_unlock(&jar_lock);
}

void cookies_set(const char* host, const char* h){
    /* "nome=valor; Domain=...; Path=...; Max-Age=0 ..." */
    const char* eq = strchr(h, '=');
    const char* semi = strchr(h, ';');
    if(!eq || (semi && eq > semi)) return;
    char name[96]; int nl = (int)(eq - h);
    while(nl > 0 && h[nl - 1] == ' ') nl--;
    if(nl <= 0 || nl >= 96) return;
    memcpy(name, h, (size_t)nl); name[nl] = 0;
    const char* v = eq + 1; int vl = semi ? (int)(semi - v) : (int)strlen(v);
    while(vl > 0 && v[vl - 1] == ' ') vl--;
    char domain[128]; snprintf(domain, sizeof domain, "%s", host);
    char path[128] = "/";
    int host_only = 1, expire = 0;
    for(const char* a = semi; a; a = strchr(a + 1, ';')){
        const char* s = a + 1; while(*s == ' ') s++;
        int al = 0; while(s[al] && s[al] != ';') al++;
        if(str_istarts(s, "domain=")){
            const char* d = s + 7; int dl = al - 7;
            if(*d == '.'){ d++; dl--; }
            if(dl > 0 && dl < 127){
                char tmp[128]; memcpy(tmp, d, (size_t)dl); tmp[dl] = 0;
                for(int i = 0; tmp[i]; i++) tmp[i] = (char)lower((u8)tmp[i]);
                Cookie probe; snprintf(probe.domain, sizeof probe.domain, "%s", tmp); probe.host_only = 0;
                if(domain_match(host, &probe)){ snprintf(domain, sizeof domain, "%s", tmp); host_only = 0; }
            }
        } else if(str_istarts(s, "path=") && al - 5 < 127){
            snprintf(path, sizeof path, "%.*s", al - 5, s + 5);
        } else if(str_istarts(s, "max-age=")){
            if(atoi(s + 8) <= 0) expire = 1;
        }
    }
    mutex_lock(&jar_lock);
    Cookie** pp = &jar;
    while(*pp){
        Cookie* c = *pp;
        if(strcmp(c->name, name) == 0 && strcmp(c->domain, domain) == 0 && strcmp(c->path, path) == 0){
            *pp = c->next; free(c->value); free(c); continue;
        }
        pp = &c->next;
    }
    if(expire){ mutex_unlock(&jar_lock); return; }
    Cookie* c = (Cookie*)calloc(1, sizeof *c);
    snprintf(c->domain, sizeof c->domain, "%s", domain);
    snprintf(c->path, sizeof c->path, "%s", path);
    snprintf(c->name, sizeof c->name, "%s", name);
    c->value = strndup(v, (size_t)vl);
    c->host_only = host_only;
    c->next = jar; jar = c;
    mutex_unlock(&jar_lock);
}

/* ---------------------------------------------------------------- HTTP */
#define MAX_BODY (12 * 1024 * 1024)

/* procura o cabecalho `name` (sem ':') no bloco de cabecalhos */
static int header_get(const char* hdr, int hlen, const char* name, char* out, int cap, int nth){
    int nl = (int)strlen(name), seen = 0;
    for(int i = 0; i < hlen; i++){
        if(i > 0 && hdr[i - 1] != '\n') continue;
        int ok = i + nl < hlen && hdr[i + nl] == ':';
        for(int k = 0; ok && k < nl; k++) if(lower((u8)hdr[i + k]) != lower((u8)name[k])) ok = 0;
        if(!ok) continue;
        if(seen++ < nth) continue;
        int a = i + nl + 1;
        while(a < hlen && (hdr[a] == ' ' || hdr[a] == '\t')) a++;
        int o = 0;
        while(a < hlen && hdr[a] != '\r' && hdr[a] != '\n' && o < cap - 1) out[o++] = hdr[a++];
        out[o] = 0;
        return 1;
    }
    return 0;
}

/* decodifica Transfer-Encoding: chunked no proprio buffer; ret novo tamanho */
static int dechunk(char* b, int n){
    int r = 0, w = 0;
    while(r < n){
        int size = 0, any = 0;
        while(r < n){
            int c = lower((u8)b[r]), d;
            if(c >= '0' && c <= '9') d = c - '0';
            else if(c >= 'a' && c <= 'f') d = c - 'a' + 10;
            else break;
            size = size * 16 + d; any = 1; r++;
        }
        while(r < n && b[r] != '\n') r++;       /* extensoes + CRLF */
        r++;
        if(!any || size == 0) break;
        if(size > n - r) size = n - r;
        memmove(b + w, b + r, (size_t)size);
        w += size; r += size;
        if(r < n && b[r] == '\r') r++;
        if(r < n && b[r] == '\n') r++;
    }
    return w;
}

/* Content-Encoding: gzip/deflate (inflate do stb_image). Comprimidas, as
 * paginas chegam com 4-8x menos bytes: menos rede e menos TLS para decifrar. */
extern char* stbi_zlib_decode_malloc(const char* buf, int len, int* outlen);
extern char* stbi_zlib_decode_noheader_malloc(const char* buf, int len, int* outlen);
static char* gunzip(const u8* b, int n, int* outlen){
    if(n < 18 || b[0] != 0x1F || b[1] != 0x8B || b[2] != 8) return NULL;
    u8 fl = b[3]; int i = 10;
    if(fl & 4){ if(i + 2 > n) return NULL; i += 2 + (b[i] | b[i + 1] << 8); }
    if(fl & 8){ while(i < n && b[i]) i++; i++; }
    if(fl & 16){ while(i < n && b[i]) i++; i++; }
    if(fl & 2) i += 2;
    if(i >= n) return NULL;
    return stbi_zlib_decode_noheader_malloc((const char*)b + i, n - i, outlen);
}
/* troca o corpo pelo descomprimido; 0 se nao deu (fica como veio) */
static int decode_body(char** p, int* n, const char* enc){
    char* out = NULL; int on = 0;
    if(str_icontains(enc, "gzip")) out = gunzip((const u8*)*p, *n, &on);
    else if(str_icontains(enc, "deflate")){
        out = stbi_zlib_decode_malloc(*p, *n, &on);
        if(!out) out = stbi_zlib_decode_noheader_malloc(*p, *n, &on);
    } else return 0;
    if(!out) return 0;
    char* r = (char*)realloc(out, (size_t)on + 1);
    if(!r){ free(out); return 0; }
    r[on] = 0;
    free(*p); *p = r; *n = on;
    return 1;
}

/* fim de uma resposta chunked ja chegou? (procura "\r\n0\r\n\r\n") */
static int chunked_done(const char* b, int n){
    if(n < 5) return 0;
    const char* tail = b + (n > 64 ? n - 64 : 0);
    for(const char* p = tail; p + 5 <= b + n; p++)
        if(p[0] == '0' && p[1] == '\r' && p[2] == '\n' && p[3] == '\r' && p[4] == '\n' && (p == b || p[-1] == '\n')) return 1;
    return 0;
}

/* ---------------------------------------------------- thread de rede
 * Cada requisicao e uma maquina de estados nao bloqueante; a thread de rede
 * avanca todas juntas (TCP e TLS sem espera) e nunca para de chamar o
 * kernel enquanto ha trafego — o kernel so processa pacotes dentro das
 * syscalls. Conexoes que terminam uma resposta bem delimitada voltam a um
 * pool (keep-alive) e servem a proxima requisicao ao mesmo host. */
#define MAXPAR 8
#define POOL 4

typedef struct { i64 sock; Tls* tls; int https, port; char host[256]; u64 since; } Conn;
static Conn pool[POOL];
static int npool = 0;

enum { J_START, J_CONNECT, J_IO, J_DONE };
typedef struct {
    Req* rq; Fetch* f;
    char cur[URLMAX]; Url u;
    const char* method; const char* body; int blen; const char* bctype;
    int hops, state, reused, retried;
    Conn c;
    Buf out; int out_done;
    Buf in; int hdr_end, clen, chunked, keep;
    u64 t0, t_act;
} Job;

volatile u32 net_gen = 1;
static Mutex q_lock = MUTEX_INIT;
static Req *q_head[2], *q_tail[2];             /* fila por prioridade */
static volatile u32 q_seq = 0;

static void conn_drop(Conn* c){
    if(c->tls){ tls_close(c->tls); c->tls = NULL; }
    if(c->sock >= 0){ sys_tcp_close(c->sock); c->sock = -1; }
}

/* conexao ociosa do pool para este destino (descarta as velhas/fechadas) */
static int pool_take(const Url* u, Conn* out){
    u64 now = sys_time_ms();
    for(int i = 0; i < npool; i++){
        Conn* c = &pool[i];
        int stale = now - c->since > 15000 || sys_tcp_status(c->sock) != 1;
        int match = !stale && c->https == u->https && c->port == u->port && !strcmp(c->host, u->host);
        if(stale || match){
            Conn got = *c;
            pool[i] = pool[--npool]; i--;
            if(stale){ conn_drop(&got); continue; }
            *out = got;
            return 1;
        }
    }
    return 0;
}
static void pool_put(Conn* c){
    if(npool == POOL){ conn_drop(&pool[0]); pool[0] = pool[--npool]; }
    c->since = sys_time_ms();
    pool[npool++] = *c;
    c->sock = -1; c->tls = NULL;
}

static void job_fail(Job* j, const char* fmt, const char* a){
    snprintf(j->f->r.err, sizeof j->f->r.err, fmt, a);
    conn_drop(&j->c);
    j->f->ok = 0; j->state = J_DONE;
}

static void job_start(Job* j){
    Resp* r = &j->f->r;
    free(r->body); r->body = NULL; r->len = 0;
    j->in.n = 0; j->out.n = 0; j->out_done = 0; j->hdr_end = -1; j->clen = -1; j->chunked = 0;
    j->t0 = j->t_act = sys_time_ms();
    if(!url_parse(j->cur, &j->u)){ job_fail(j, "Endereço inválido: %s", j->cur); return; }
    Url* u = &j->u;
    buf_printf(&j->out, "%s %s HTTP/1.1\r\nHost: %s", j->method, u->path, u->host);
    if(u->port != (u->https ? 443 : 80)) buf_printf(&j->out, ":%d", u->port);
    buf_puts(&j->out, "\r\nUser-Agent: Mozilla/5.0 (Pulsar OS) Vela/1.0\r\n"
                      "Accept: text/html,application/xhtml+xml,*/*;q=0.8\r\n"
                      "Accept-Language: pt-BR,pt;q=0.9,en;q=0.8\r\n"
                      "Accept-Encoding: gzip, deflate\r\nConnection: keep-alive\r\n");
    Buf ck = {0};
    cookies_get(u->host, u->path, &ck);
    if(ck.n) buf_printf(&j->out, "Cookie: %s\r\n", ck.p);
    buf_free(&ck);
    if(j->body) buf_printf(&j->out, "Content-Type: %s\r\nContent-Length: %d\r\n", j->bctype ? j->bctype : "application/x-www-form-urlencoded", j->blen);
    buf_puts(&j->out, "\r\n");
    if(j->body) buf_put(&j->out, j->body, j->blen);
    j->reused = pool_take(u, &j->c);
    if(j->reused){ j->state = J_IO; return; }
    u32 ip;
    if(!dns_resolve(u->host, &ip)){ job_fail(j, "Não foi possível resolver %s (DNS).", u->host); return; }
    j->c.sock = sys_tcp_connect(ip, (u16)u->port);
    j->c.tls = NULL; j->c.https = u->https; j->c.port = u->port;
    snprintf(j->c.host, sizeof j->c.host, "%s", u->host);
    if(j->c.sock < 0){ job_fail(j, "Falha ao conectar a %s.", u->host); return; }
    j->state = J_CONNECT;
}

/* a resposta esta completa? */
static int job_complete(Job* j, int eof){
    if(j->hdr_end < 0){
        for(int i = 3; i < j->in.n; i++)
            if(j->in.p[i] == '\n' && j->in.p[i - 1] == '\r' && j->in.p[i - 2] == '\n' && j->in.p[i - 3] == '\r'){ j->hdr_end = i + 1; break; }
        if(j->hdr_end >= 0){
            char v[64];
            if(header_get(j->in.p, j->hdr_end, "content-length", v, sizeof v, 0)) j->clen = atoi(v);
            if(header_get(j->in.p, j->hdr_end, "transfer-encoding", v, sizeof v, 0) && str_icontains(v, "chunked")) j->chunked = 1;
            const char* sp = strchr(j->in.p, ' ');
            int code = sp ? atoi(sp + 1) : 0;
            if(!strcmp(j->method, "HEAD") || code == 204 || code == 304 || (code >= 100 && code < 200)) j->clen = 0;
            j->keep = (j->clen >= 0 || j->chunked)
                && !(header_get(j->in.p, j->hdr_end, "connection", v, sizeof v, 0) && str_icontains(v, "close"));
        }
    }
    if(j->hdr_end < 0) return eof;
    int bl = j->in.n - j->hdr_end;
    if(j->clen >= 0 && bl >= j->clen) return 1;
    if(j->chunked && chunked_done(j->in.p + j->hdr_end, bl)) return 1;
    if(bl > MAX_BODY){ j->keep = 0; return 1; }
    return eof;
}

static void job_finish(Job* j, int eof){
    Resp* r = &j->f->r;
    if(j->hdr_end < 0){
        /* conexao reaproveitada que o servidor ja tinha fechado: tenta de novo */
        if(j->reused && !j->retried && j->in.n == 0){ conn_drop(&j->c); job_start(j); j->retried = 1; return; }
        job_fail(j, "%s", j->in.n ? "Resposta HTTP inválida." : "A conexão falhou ou expirou.");
        return;
    }
    if(j->keep && !eof) pool_put(&j->c); else conn_drop(&j->c);
    char* p = j->in.p;
    const char* sp = strchr(p, ' ');
    r->status = sp ? atoi(sp + 1) : 0;
    header_get(p, j->hdr_end, "content-type", r->ctype, sizeof r->ctype, 0);
    char loc[URLMAX]; loc[0] = 0;
    header_get(p, j->hdr_end, "location", loc, sizeof loc, 0);
    char sc[1024];
    for(int k = 0; header_get(p, j->hdr_end, "set-cookie", sc, sizeof sc, k); k++) cookies_set(j->u.host, sc);
    char enc[64]; enc[0] = 0;
    header_get(p, j->hdr_end, "content-encoding", enc, sizeof enc, 0);
    int bl = j->in.n - j->hdr_end;
    if(j->clen >= 0 && bl > j->clen) bl = j->clen;
    memmove(p, p + j->hdr_end, (size_t)bl);
    if(j->chunked) bl = dechunk(p, bl);
    p[bl] = 0;
    int zbytes = 0;
    if(enc[0] && bl > 0 && r->status != 304){ zbytes = bl; if(!decode_body(&p, &bl, enc)) zbytes = 0; }
    r->body = p; r->len = bl;
    j->in.p = NULL; j->in.n = j->in.cap = 0;
    url_format(&j->u, r->url, sizeof r->url);
    printf("[vela] %s %s%s -> %d, %d B%s, %u ms%s\n", j->method, j->u.host, j->u.path,
           r->status, r->len, zbytes ? " (gzip)" : "", (unsigned)(sys_time_ms() - j->t0), j->reused ? " (keep-alive)" : "");
    if(r->status >= 300 && r->status < 400 && r->status != 304 && loc[0] && j->hops < 8){
        char next[URLMAX];
        if(url_resolve(r->url, loc, next, sizeof next)){
            char* h = strchr(next, '#'); if(h) *h = 0;
            snprintf(j->cur, sizeof j->cur, "%s", next);
            if(r->status != 307 && r->status != 308){ j->method = "GET"; j->body = NULL; j->blen = 0; }
            j->hops++; j->reused = j->retried = 0;
            job_start(j);
            return;
        }
    }
    j->f->ok = 1; j->state = J_DONE;
}

/* um passo sem bloquear; 1 se algo andou */
static int job_step(Job* j){
    int prog = 0;
    u64 now = sys_time_ms();
    if(j->state == J_CONNECT){
        i64 st = sys_tcp_status(j->c.sock);
        if(st == 1){
            j->state = J_IO; prog = 1; j->t_act = now;
            if(j->u.https && !(j->c.tls = tls_new(j->u.host))){ job_fail(j, "Falha ao iniciar o TLS com %s.", j->u.host); return 1; }
        } else if(st < 0 || st == 2 || now - j->t_act > 10000){ job_fail(j, "Falha ao conectar a %s.", j->u.host); return 1; }
        return prog;
    }
    if(j->state != J_IO) return 0;
    int eof = 0;
    if(j->c.tls){
        int r = tls_io(j->c.tls, j->c.sock, (const u8*)j->out.p, j->out.n, &j->out_done, &j->in, &prog);
        if(r < 0){
            if(j->reused && !j->retried && j->in.n == 0){ conn_drop(&j->c); job_start(j); j->retried = 1; return 1; }
            int e = tls_error(j->c.tls);
            char m[200]; snprintf(m, sizeof m, "%s: %s (erro %d)", j->u.host, tls_error_text(e), e);
            job_fail(j, "HTTPS falhou em %s.", m);
            return 1;
        }
        if(r == 1) eof = 1;
    } else {
        while(j->out_done < j->out.n){
            i64 w = sys_tcp_send(j->c.sock, j->out.p + j->out_done, (u64)(j->out.n - j->out_done));
            if(w < 0){ eof = 1; break; }
            if(w == 0) break;
            j->out_done += (int)w; prog = 1;
        }
        for(int k = 0; k < 8; k++){
            char tmp[16384];
            i64 r = sys_tcp_recv(j->c.sock, tmp, sizeof tmp);
            if(r > 0){ buf_put(&j->in, tmp, (int)r); prog = 1; continue; }
            if(r == 0 || r == -1) eof = 1;
            break;
        }
    }
    if(prog) j->t_act = now;
    else if(now - j->t_act > 20000) eof = 1;
    if(job_complete(j, eof)) job_finish(j, eof);
    return prog || eof;
}

/* ------------------------------------------------- pedidos (qualquer thread) */
Req* req_new(const char* url, int prio){
    Req* r = (Req*)calloc(1, sizeof *r);
    snprintf(r->url, sizeof r->url, "%s", url);
    r->f.url = r->url;
    r->prio = prio ? 1 : 0;
    r->gen = __atomic_load_n(&net_gen, __ATOMIC_ACQUIRE);
    return r;
}
void req_free(Req* r){ if(!r) return; resp_free(&r->f.r); free(r); }

void net_submit(Req* r){
    if(r->task.done) work_hold();          /* a thread principal vai receber */
    mutex_lock(&q_lock);
    r->qnext = NULL;
    if(q_tail[r->prio]) q_tail[r->prio]->qnext = r; else q_head[r->prio] = r;
    q_tail[r->prio] = r;
    __atomic_add_fetch(&q_seq, 1, __ATOMIC_RELEASE);
    mutex_unlock(&q_lock);
    sys_futex_wake(&q_seq, 1);
}

void net_wait(Req* r){
    while(!__atomic_load_n(&r->fin, __ATOMIC_ACQUIRE)) sys_futex_wait(&r->fin, 0, 0);
}

/* a rede terminou: worker (se houver etapa pesada) ou direto para a principal */
static void req_complete(Req* r){
    int has_done = r->task.done != NULL;
    void (*run)(Task*) = r->task.run;
    __atomic_store_n(&r->fin, 1, __ATOMIC_RELEASE);
    sys_futex_wake(&r->fin, 8);
    /* depois de fin=1 um pedido sincrono pode ja ter sido liberado: so toca
     * em `r` se ele tem entrega assincrona */
    if(!has_done) return;
    if(run && r->f.ok) work_submit(&r->task);
    else work_post_main(&r->task);
}

static Req* q_pop(void){
    mutex_lock(&q_lock);
    Req* r = NULL;
    for(int p = 0; p < 2 && !r; p++){
        r = q_head[p];
        if(r){ q_head[p] = r->qnext; if(!q_head[p]) q_tail[p] = NULL; }
    }
    mutex_unlock(&q_lock);
    return r;
}

static void net_thread(void* arg){
    (void)arg;
    Job jobs[MAXPAR];
    int active = 0;
    memset(jobs, 0, sizeof jobs);
    for(;;){
        u32 seen = __atomic_load_n(&q_seq, __ATOMIC_ACQUIRE);
        /* novos pedidos ate encher as vagas */
        while(active < MAXPAR){
            Req* r = q_pop();
            if(!r) break;
            if(r->gen && r->gen != __atomic_load_n(&net_gen, __ATOMIC_ACQUIRE)){
                /* pagina abandonada antes de comecar: nem baixa */
                snprintf(r->f.r.err, sizeof r->f.r.err, "cancelado");
                r->f.ok = 0;
                req_complete(r);
                continue;
            }
            Job* j = &jobs[active++];
            Buf in = j->in, out = j->out;               /* reaproveita os buffers */
            memset(j, 0, sizeof *j);
            j->in = in; j->out = out;
            j->rq = r; j->f = &r->f; j->c.sock = -1;
            snprintf(j->cur, sizeof j->cur, "%s", r->f.url);
            char* h = strchr(j->cur, '#'); if(h) *h = 0;
            j->method = r->f.method ? r->f.method : "GET";
            j->body = r->f.body; j->blen = r->f.body_len; j->bctype = r->f.body_ctype;
            memset(&r->f.r, 0, sizeof r->f.r); r->f.ok = 0;
            job_start(j);
        }
        if(!active){ sys_futex_wait(&q_seq, seen, 0); continue; }
        int prog = 0;
        for(int i = 0; i < active; i++){
            Job* j = &jobs[i];
            if(j->state != J_DONE) prog |= job_step(j);
            if(j->state == J_DONE){
                Req* r = j->rq;
                conn_drop(&j->c);
                Job tmp = jobs[i]; jobs[i] = jobs[active - 1]; jobs[active - 1] = tmp;
                active--; i--;
                req_complete(r);
                prog = 1;
            }
        }
        if(!prog) sys_sleep(1);
    }
}

void net_init(void){
    thread_spawn(net_thread, NULL, 256 * 1024);
}

/* --------------------------------------------------------- versoes sincronas */
void net_fetch_many(Fetch* f, int n){
    Req* rs = (Req*)calloc((size_t)(n > 0 ? n : 1), sizeof(Req));
    for(int i = 0; i < n; i++){
        snprintf(rs[i].url, sizeof rs[i].url, "%s", f[i].url);
        rs[i].f = f[i]; rs[i].f.url = rs[i].url;
        net_submit(&rs[i]);
    }
    for(int i = 0; i < n; i++){
        net_wait(&rs[i]);
        f[i].r = rs[i].f.r; f[i].ok = rs[i].f.ok;
    }
    free(rs);
}

int net_fetch(const char* url, const char* method, const char* body, int body_len, const char* body_ctype, Resp* r){
    Fetch f = { url, method, body, body_len, body_ctype, {0}, 0 };
    net_fetch_many(&f, 1);
    *r = f.r;
    return f.ok;
}

void resp_free(Resp* r){ free(r->body); r->body = NULL; r->len = 0; }
