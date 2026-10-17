/* time.c — relogio de parede (RTC + contador em ms); fuso sempre UTC. */
#include <time.h>
#include <sys/time.h>
#include "pulsar.h"

/* o RTC tem resolucao de segundos: ancora os ms do contador no primeiro uso */
static u64 base_ms = 0, base_rtc = 0;
static u64 now_unix_ms(void){
    if(!base_rtc){ base_rtc = sys_rtc(); base_ms = sys_time_ms(); }
    return base_rtc * 1000 + (sys_time_ms() - base_ms);
}
time_t time(time_t* t){ time_t v = (time_t)(now_unix_ms() / 1000); if(t) *t = v; return v; }
clock_t clock(void){ return (clock_t)sys_time_ms(); }
int clock_gettime(clockid_t id, struct timespec* ts){
    u64 ms = id == CLOCK_MONOTONIC ? sys_time_ms() : now_unix_ms();
    ts->tv_sec = (time_t)(ms / 1000); ts->tv_nsec = (long)(ms % 1000) * 1000000;
    return 0;
}
int gettimeofday(struct timeval* tv, void* tz){
    (void)tz;
    u64 ms = now_unix_ms();
    tv->tv_sec = (time_t)(ms / 1000); tv->tv_usec = (long)(ms % 1000) * 1000;
    return 0;
}
struct tm* gmtime_r(const time_t* t, struct tm* o){
    u32 y, mo, d, h, mi, s, wd;
    civil_time((u64)*t, &y, &mo, &d, &h, &mi, &s, &wd);
    o->tm_year = (int)y - 1900; o->tm_mon = (int)mo - 1; o->tm_mday = (int)d;
    o->tm_hour = (int)h; o->tm_min = (int)mi; o->tm_sec = (int)s; o->tm_wday = (int)wd;
    static const int cum[12] = {0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334};
    int leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    o->tm_yday = cum[mo - 1] + (int)d - 1 + (leap && mo > 2);
    o->tm_isdst = 0; o->tm_gmtoff = 0; o->tm_zone = "UTC";
    return o;
}
struct tm* localtime_r(const time_t* t, struct tm* o){ return gmtime_r(t, o); }
static struct tm tm_static;
struct tm* gmtime(const time_t* t){ return gmtime_r(t, &tm_static); }
struct tm* localtime(const time_t* t){ return gmtime_r(t, &tm_static); }
time_t mktime(struct tm* tm){
    /* dias desde a epoca (algoritmo de Howard Hinnant) */
    long y = tm->tm_year + 1900, m = tm->tm_mon + 1;
    y -= m <= 2;
    long era = (y >= 0 ? y : y - 399) / 400;
    long yoe = y - era * 400;
    long doy = (153 * (m + (m > 2 ? -3 : 9)) + 2) / 5 + tm->tm_mday - 1;
    long doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    long days = era * 146097 + doe - 719468;
    return days * 86400 + tm->tm_hour * 3600 + tm->tm_min * 60 + tm->tm_sec;
}
