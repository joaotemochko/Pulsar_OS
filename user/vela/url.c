/* url.c — URLs http(s): analise, resolucao de relativas (RFC 3986 5.2)
 * e o que o usuario digita na barra de endereco. */
#include "vela.h"

static int is_ip_literal(const char* h){
    int dots = 0;
    for(; *h; h++){ if(*h == '.') dots++; else if(*h < '0' || *h > '9') return 0; }
    return dots == 3;
}

int url_parse(const char* s, Url* u){
    memset(u, 0, sizeof *u);
    while(*s == ' ') s++;
    if(str_istarts(s, "https://")){ u->https = 1; s += 8; }
    else if(str_istarts(s, "http://")){ s += 7; }
    else return 0;
    /* autoridade: [user@]host[:porta] */
    const char* a = s;
    while(*s && *s != '/' && *s != '?' && *s != '#') s++;
    const char* at = NULL;
    for(const char* p = a; p < s; p++) if(*p == '@') at = p;
    if(at) a = at + 1;
    int hn = 0;
    const char* p = a;
    while(p < s && *p != ':' && hn < 255) u->host[hn++] = (char)lower((u8)*p++);
    u->host[hn] = 0;
    if(!hn) return 0;
    u->port = u->https ? 443 : 80;
    if(p < s && *p == ':'){
        p++;
        if(p < s){
            int port = 0;
            while(p < s && *p >= '0' && *p <= '9') port = port * 10 + (*p++ - '0');
            if(port <= 0 || port > 65535) return 0;
            u->port = port;
        }
    }
    int pn = 0;
    if(*s != '/') u->path[pn++] = '/';
    while(*s && *s != '#' && pn < URLMAX - 1){
        char c = *s++;
        if(c == ' '){                 /* espaco literal: codifica */
            if(pn + 3 >= URLMAX) break;
            u->path[pn++] = '%'; u->path[pn++] = '2'; u->path[pn++] = '0';
        } else if(c != '\n' && c != '\r' && c != '\t') u->path[pn++] = c;
    }
    u->path[pn] = 0;
    if(*s == '#'){
        int fn = 0; s++;
        while(*s && fn < 255) u->frag[fn++] = *s++;
        u->frag[fn] = 0;
    }
    return 1;
}

void url_format(const Url* u, char* out, int cap){
    int def = u->https ? 443 : 80;
    if(u->port != def) snprintf(out, (size_t)cap, "%s://%s:%d%s", u->https ? "https" : "http", u->host, u->port, u->path);
    else snprintf(out, (size_t)cap, "%s://%s%s", u->https ? "https" : "http", u->host, u->path);
}

/* remove "." e ".." de um caminho (sem a query) */
static void remove_dots(char* path){
    char out[URLMAX]; int o = 0;
    const char* in = path;
    while(*in){
        if(strncmp(in, "../", 3) == 0) in += 3;
        else if(strncmp(in, "./", 2) == 0) in += 2;
        else if(strncmp(in, "/./", 3) == 0) in += 2;
        else if(strcmp(in, "/.") == 0){ in += 1; ((char*)in)[0] = '/'; }
        else if(strncmp(in, "/../", 4) == 0 || strcmp(in, "/..") == 0){
            in += 3;
            if(*in == 0){ in = "/"; }
            while(o > 0 && out[o - 1] != '/') o--;
            if(o > 0) o--;
        }
        else if(strcmp(in, ".") == 0 || strcmp(in, "..") == 0) break;
        else {
            if(*in == '/' && o < URLMAX - 1) out[o++] = *in++;
            while(*in && *in != '/' && o < URLMAX - 1) out[o++] = *in++;
        }
    }
    out[o] = 0;
    memcpy(path, out, (size_t)o + 1);
}

int url_resolve(const char* base, const char* ref, char* out, int cap){
    while(*ref == ' ' || *ref == '\n' || *ref == '\t') ref++;
    if(str_istarts(ref, "http://") || str_istarts(ref, "https://")){
        Url u; if(!url_parse(ref, &u)) return 0;
        url_format(&u, out, cap);
        if(u.frag[0]){ int l = (int)strlen(out); snprintf(out + l, (size_t)(cap - l), "#%s", u.frag); }
        return 1;
    }
    /* outros esquemas (mailto:, javascript:, data:) passam como estao */
    for(const char* p = ref; *p && *p != '/' && *p != '?' && *p != '#'; p++)
        if(*p == ':'){ snprintf(out, (size_t)cap, "%s", ref); return 1; }
    Url b; if(!url_parse(base, &b)) return 0;
    if(ref[0] == '/' && ref[1] == '/'){
        char tmp[URLMAX]; snprintf(tmp, sizeof tmp, "%s:%s", b.https ? "https" : "http", ref);
        return url_resolve(base, tmp, out, cap);
    }
    Url r = b; r.frag[0] = 0;
    const char* hash = strchr(ref, '#');
    int rlen = hash ? (int)(hash - ref) : (int)strlen(ref);
    if(hash) snprintf(r.frag, sizeof r.frag, "%s", hash + 1);
    if(rlen == 0){
        /* so fragmento: mesmo documento */
    } else if(ref[0] == '?'){
        char* q = strchr(r.path, '?'); if(q) *q = 0;
        int l = (int)strlen(r.path);
        snprintf(r.path + l, sizeof r.path - (size_t)l, "%.*s", rlen, ref);
    } else {
        char merged[URLMAX];
        if(ref[0] == '/') snprintf(merged, sizeof merged, "%.*s", rlen, ref);
        else {
            char* q = strchr(r.path, '?'); if(q) *q = 0;
            char* slash = strrchr(r.path, '/');
            int dl = slash ? (int)(slash - r.path) + 1 : 0;
            snprintf(merged, sizeof merged, "%.*s%.*s", dl, r.path, rlen, ref);
        }
        char* q = strchr(merged, '?');
        char query[URLMAX] = "";
        if(q){ snprintf(query, sizeof query, "%s", q); *q = 0; }
        remove_dots(merged);
        if(!merged[0]) strcpy(merged, "/");
        snprintf(r.path, sizeof r.path, "%s%s", merged, query);
    }
    url_format(&r, out, cap);
    if(r.frag[0]){ int l = (int)strlen(out); snprintf(out + l, (size_t)(cap - l), "#%s", r.frag); }
    return 1;
}

/* o que foi digitado na barra -> URL absoluta. Sem esquema: IPs e portas
 * explicitas usam http (servidores locais); o resto, https. */
void url_from_input(const char* t, char* out, int cap){
    while(*t == ' ') t++;
    if(str_istarts(t, "http://") || str_istarts(t, "https://") || str_istarts(t, "about:")){
        snprintf(out, (size_t)cap, "%s", t); return;
    }
    char host[256]; int n = 0;
    while(t[n] && t[n] != '/' && t[n] != ':' && t[n] != '?' && n < 255){ host[n] = t[n]; n++; }
    host[n] = 0;
    int has_port = t[n] == ':';
    int local = is_ip_literal(host) || has_port || strcmp(host, "localhost") == 0;
    if(!strchr(host, '.') && !local){
        /* uma palavra solta: pesquisa (DuckDuckGo Lite, que dispensa JS) */
        Buf b = {0};
        buf_puts(&b, "https://lite.duckduckgo.com/lite/?q=");
        url_encode_component(&b, t, (int)strlen(t));
        snprintf(out, (size_t)cap, "%s", buf_cstr(&b));
        buf_free(&b);
        return;
    }
    snprintf(out, (size_t)cap, "%s://%s", local ? "http" : "https", t);
}

void url_encode_component(Buf* b, const char* s, int n){
    static const char* hex = "0123456789ABCDEF";
    for(int i = 0; i < n; i++){
        u8 c = (u8)s[i];
        if((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '-' || c == '_' || c == '.' || c == '~')
            buf_putc(b, (char)c);
        else if(c == ' ') buf_putc(b, '+');
        else { buf_putc(b, '%'); buf_putc(b, hex[c >> 4]); buf_putc(b, hex[c & 15]); }
    }
}
