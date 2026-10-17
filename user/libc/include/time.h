/* time.h — relogio de parede pelo RTC; fuso sempre UTC. */
#ifndef _TIME_H
#define _TIME_H
#include <stddef.h>
typedef long time_t;
typedef long clock_t;
struct tm {
    int tm_sec, tm_min, tm_hour, tm_mday, tm_mon, tm_year, tm_wday, tm_yday, tm_isdst;
    long tm_gmtoff;
    const char* tm_zone;
};
struct timespec { time_t tv_sec; long tv_nsec; };
#define CLOCK_REALTIME  0
#define CLOCK_MONOTONIC 1
#define CLOCKS_PER_SEC 1000
typedef int clockid_t;
time_t time(time_t* t);
clock_t clock(void);
int clock_gettime(clockid_t id, struct timespec* ts);
struct tm* gmtime_r(const time_t* t, struct tm* out);
struct tm* localtime_r(const time_t* t, struct tm* out);
struct tm* gmtime(const time_t* t);
struct tm* localtime(const time_t* t);
time_t mktime(struct tm* tm);
#endif
