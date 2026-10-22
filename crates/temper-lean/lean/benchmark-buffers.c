/* Preload every pinned input before warmup/timing. No generation or file I/O in
   the measured interval. Print independent samples per workload, not one sum. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
typedef struct { const unsigned char *data; size_t len; } bytes;
typedef struct { uint64_t parameter; size_t count; bytes *files; } workload;
extern int temper_initialize(void);
extern void *temper_buffers_call(uint64_t, const bytes *, size_t);
extern int temper_buffers_status(const void *);
extern size_t temper_buffers_count(const void *);
extern const unsigned char *temper_buffers_view(const void *, size_t, size_t *);
extern void temper_buffers_free(void *);
static volatile uint64_t consumed;
static uint64_t number(FILE *f) {
    unsigned char b[8]; if (fread(b,1,8,f)!=8) exit(2);
    uint64_t n=0; for(int i=7;i>=0;--i) n=(n<<8)|b[i]; return n;
}
static double now_ns(clockid_t clock) {
    struct timespec t; if(clock_gettime(clock,&t)) exit(3);
    return (double)t.tv_sec*1000000000.0+(double)t.tv_nsec;
}
static void invoke(const workload *w) {
    void *r=temper_buffers_call(w->parameter,w->files,w->count);
    if(!r || temper_buffers_status(r)) exit(10);
    size_t count=temper_buffers_count(r);
    for(size_t i=0;i<count;++i) {
        size_t size; const unsigned char *view=temper_buffers_view(r,i,&size);
        if(!view) exit(11);
        consumed+=size;
        if(size) consumed+=view[0]+view[size-1];
    }
    temper_buffers_free(r);
}
int main(int argc,char **argv) {
    if(argc!=2 || temper_initialize()) return 4;
    FILE *f=fopen(argv[1],"rb"); if(!f) return 5;
    uint64_t samples=number(f),iterations=number(f),count=number(f),cpu=number(f);
    if(!samples || samples>1000 || !iterations || iterations>1000000 || !count || count>1024) return 6;
    workload *work=calloc(count,sizeof(workload)); if(!work) return 7;
    for(size_t i=0;i<count;++i) {
        work[i].parameter=number(f); work[i].count=number(f);
        work[i].files=calloc(work[i].count,sizeof(bytes)); if(!work[i].files) return 7;
        for(size_t j=0;j<work[i].count;++j) {
            size_t size=number(f); unsigned char *data=malloc(size?size:1);
            if(!data || fread(data,1,size,f)!=size) return 8;
            work[i].files[j]=(bytes){data,size};
        }
    }
    fclose(f);
    for(size_t i=0;i<count;++i) invoke(&work[i]);
    clockid_t clock=cpu?CLOCK_PROCESS_CPUTIME_ID:CLOCK_MONOTONIC;
    for(size_t sample=0;sample<samples;++sample) {
        /* Rotate order to reduce systematic ordering/temperature bias. */
        for(size_t offset=0;offset<count;++offset) {
            size_t i=(offset+sample)%count;
            double start=now_ns(clock);
            for(size_t n=0;n<iterations;++n) invoke(&work[i]);
            double elapsed=now_ns(clock)-start;
            printf("%zu %.3f\n",i,elapsed/(double)iterations);
        }
    }
    for(size_t i=0;i<count;++i) { for(size_t j=0;j<work[i].count;++j) free((void*)work[i].files[j].data); free(work[i].files); }
    free(work); return 0;
}
