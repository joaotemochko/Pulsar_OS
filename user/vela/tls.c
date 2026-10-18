/* tls.c — HTTPS com o BearSSL (TLS 1.0-1.2, validacao X.509 pelas raizes
 * da Mozilla em certs.c). A entropia vem de SYS_GETRANDOM (virtio-rng) e a
 * hora para checar validade dos certificados, do RTC.
 *
 * Interface nao bloqueante: tls_io() empurra o motor do BearSSL o quanto
 * der sem esperar (o socket e nao bloqueante), o que permite varias
 * conexoes HTTPS em paralelo num laco so. Sessoes ficam guardadas por host:
 * a proxima conexao ao mesmo servidor pula a troca de chaves. */
#include "vela.h"
#include "bearssl.h"

extern const br_x509_trust_anchor* const vela_tas;
extern const unsigned vela_tas_num;

struct Tls {
    br_ssl_client_context sc;
    br_x509_minimal_context xc;
    char host[256];
    int ready;                  /* handshake concluido */
    int ioerr;                  /* erro do socket (o BearSSL nao sabe dele) */
    unsigned char iobuf[BR_SSL_BUFSIZE_BIDI];
};

static struct { char host[128]; br_ssl_session_parameters p; } sessions[24];
static int nsessions = 0;

const char* tls_error_text(int e){
    switch(e){
    case BR_ERR_X509_EXPIRED: return "certificado expirado ou ainda não válido (confira o relógio)";
    case BR_ERR_X509_NOT_TRUSTED: return "certificado emitido por autoridade não confiável";
    case BR_ERR_X509_BAD_SERVER_NAME: return "o certificado não pertence a este site";
    case BR_ERR_X509_UNSUPPORTED: case BR_ERR_X509_WEAK_PUBLIC_KEY: return "certificado com algoritmo não suportado";
    case BR_ERR_BAD_VERSION: case BR_ERR_UNSUPPORTED_VERSION: return "versão de TLS não suportada pelo servidor";
    case BR_ERR_BAD_CIPHER_SUITE: return "nenhuma cifra em comum com o servidor";
    case BR_ERR_IO: return "conexão encerrada durante o TLS";
    case BR_ERR_RECV_FATAL_ALERT + 70: return "o servidor exige TLS 1.3 (o Vela fala até TLS 1.2)";
    case BR_ERR_RECV_FATAL_ALERT + 40: return "o servidor recusou o handshake (provavelmente exige TLS 1.3)";
    }
    if(e >= BR_ERR_RECV_FATAL_ALERT && e < BR_ERR_SEND_FATAL_ALERT) return "o servidor abortou o TLS";
    if(e >= BR_ERR_X509_OK) return "certificado inválido";
    return "erro no protocolo TLS";
}

Tls* tls_new(const char* host){
    static int warned = 0;
    if(!sys_getrandom(NULL, 0) && !warned){
        warned = 1;
        puts_("[vela] AVISO: sem virtio-rng; chaves TLS com entropia fraca\n");
    }
    Tls* t = (Tls*)calloc(1, sizeof *t);
    if(!t) return NULL;
    snprintf(t->host, sizeof t->host, "%s", host);
    br_ssl_client_init_full(&t->sc, &t->xc, vela_tas, vela_tas_num);
    /* ChaCha20-Poly1305 primeiro: em software e bem mais rapido que AES */
    static const uint16_t SUITES[] = {
        BR_TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256, BR_TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
        BR_TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256, BR_TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
        BR_TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384, BR_TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
        BR_TLS_ECDHE_ECDSA_WITH_AES_128_CBC_SHA256, BR_TLS_ECDHE_RSA_WITH_AES_128_CBC_SHA256,
        BR_TLS_ECDHE_ECDSA_WITH_AES_128_CBC_SHA, BR_TLS_ECDHE_RSA_WITH_AES_128_CBC_SHA,
        BR_TLS_ECDHE_ECDSA_WITH_AES_256_CBC_SHA, BR_TLS_ECDHE_RSA_WITH_AES_256_CBC_SHA,
        BR_TLS_RSA_WITH_AES_128_GCM_SHA256, BR_TLS_RSA_WITH_AES_256_GCM_SHA384,
        BR_TLS_RSA_WITH_AES_128_CBC_SHA256, BR_TLS_RSA_WITH_AES_128_CBC_SHA, BR_TLS_RSA_WITH_AES_256_CBC_SHA,
    };
    br_ssl_engine_set_suites(&t->sc.eng, SUITES, sizeof SUITES / sizeof SUITES[0]);
    br_ssl_engine_set_buffer(&t->sc.eng, t->iobuf, sizeof t->iobuf, 1);
    unsigned char seed[48];
    sys_getrandom(seed, sizeof seed);
    br_ssl_engine_inject_entropy(&t->sc.eng, seed, sizeof seed);
    u64 now = sys_rtc();
    br_x509_minimal_set_time(&t->xc, (uint32_t)(now / 86400 + 719528), (uint32_t)(now % 86400));
    int resume = 0;
    for(int i = 0; i < nsessions; i++)
        if(!strcmp(sessions[i].host, host)){ br_ssl_engine_set_session_parameters(&t->sc.eng, &sessions[i].p); resume = 1; break; }
    if(!br_ssl_client_reset(&t->sc, host, resume)){ free(t); return NULL; }
    return t;
}

static void save_session(Tls* t){
    int slot = -1;
    for(int i = 0; i < nsessions; i++) if(!strcmp(sessions[i].host, t->host)) slot = i;
    if(slot < 0) slot = nsessions < 24 ? nsessions++ : (int)(sys_time_ms() % 24);
    snprintf(sessions[slot].host, sizeof sessions[slot].host, "%s", t->host);
    br_ssl_engine_get_session_parameters(&t->sc.eng, &sessions[slot].p);
}

int tls_ready(Tls* t){ return t->ready; }
int tls_has_session(const char* host){
    for(int i = 0; i < nsessions; i++) if(!strcmp(sessions[i].host, host)) return 1;
    return 0;
}

int tls_error(Tls* t){ int e = br_ssl_engine_last_error(&t->sc.eng); return e ? e : t->ioerr; }

/* Avanca o motor sem bloquear: envia o que der de `out` (a partir de
 * *done), poe o que chegar em `in`. *prog = 1 se algo andou.
 * Retorna 0 (continue depois), 1 (conexao terminou) ou -1 (erro). */
int tls_io(Tls* t, i64 sock, const u8* out, int outn, int* done, Buf* in, int* prog){
    br_ssl_engine_context* e = &t->sc.eng;
    for(int guard = 0; guard < 64; guard++){
        unsigned st = br_ssl_engine_current_state(e);
        if(st == BR_SSL_CLOSED){
            int err = br_ssl_engine_last_error(e);
            return err == BR_ERR_OK || (err == BR_ERR_IO && t->ready) ? 1 : -1;
        }
        if(!t->ready && (st & BR_SSL_SENDAPP)){
            t->ready = 1; save_session(t);
            br_ssl_session_parameters sp; br_ssl_engine_get_session_parameters(e, &sp);
            printf("[vela] TLS %s: cifra %04x\n", t->host, sp.cipher_suite);
        }
        if(st & BR_SSL_SENDREC){
            size_t len; unsigned char* b = br_ssl_engine_sendrec_buf(e, &len);
            i64 w = sys_tcp_send(sock, b, len);
            if(w < 0){ t->ioerr = BR_ERR_IO; return -1; }
            if(w > 0){ br_ssl_engine_sendrec_ack(e, (size_t)w); *prog = 1; continue; }
        }
        if(st & BR_SSL_RECVAPP){
            size_t len; unsigned char* b = br_ssl_engine_recvapp_buf(e, &len);
            buf_put(in, b, (int)len);
            br_ssl_engine_recvapp_ack(e, len);
            *prog = 1;
            continue;
        }
        if((st & BR_SSL_SENDAPP) && *done < outn){
            size_t len; unsigned char* b = br_ssl_engine_sendapp_buf(e, &len);
            size_t n = (size_t)(outn - *done) < len ? (size_t)(outn - *done) : len;
            memcpy(b, out + *done, n);
            br_ssl_engine_sendapp_ack(e, n);
            *done += (int)n;
            if(*done == outn) br_ssl_engine_flush(e, 0);
            *prog = 1;
            continue;
        }
        if(st & BR_SSL_RECVREC){
            size_t len; unsigned char* b = br_ssl_engine_recvrec_buf(e, &len);
            i64 r = sys_tcp_recv(sock, b, len);
            if(r > 0){ br_ssl_engine_recvrec_ack(e, (size_t)r); *prog = 1; continue; }
            if(r == 0 || r == -1){
                /* o par fechou o TCP (muitos servidores nao mandam close_notify) */
                if(!t->ready) t->ioerr = BR_ERR_IO;
                return t->ready ? 1 : -1;
            }
        }
        break;
    }
    return 0;
}

void tls_close(Tls* t){ free(t); }
