#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

extern int temper_initialize(void);
extern char *temper_call(const char *, size_t, size_t *);
extern void temper_free(char *);

static uint64_t number(FILE *file) {
    unsigned char data[8];
    if (fread(data, 1, 8, file) != 8) exit(2);
    uint64_t n = 0;
    for (int i = 7; i >= 0; --i) n = (n << 8) | data[i];
    return n;
}
static double now_ns(void) {
    struct timespec time;
    if (clock_gettime(CLOCK_MONOTONIC, &time)) exit(3);
    return (double)time.tv_sec * 1000000000.0 + time.tv_nsec;
}
int main(int argc, char **argv) {
    if (argc != 2 || temper_initialize()) return 4;
    FILE *file = fopen(argv[1], "rb");
    if (!file) return 5;
    uint64_t samples = number(file), iterations = number(file), count = number(file);
    if (!samples || samples > 1000 || !iterations || iterations > 1000000 || !count || count > 1024) return 6;
    char **inputs = calloc(count, sizeof(char *));
    size_t *sizes = calloc(count, sizeof(size_t));
    if (!inputs || !sizes) return 7;
    for (uint64_t i = 0; i < count; ++i) {
        sizes[i] = number(file);
        if (sizes[i] > 16777216) return 8;
        inputs[i] = malloc(sizes[i] + 1);
        if (!inputs[i] || fread(inputs[i], 1, sizes[i], file) != sizes[i]) return 9;
    }
    fclose(file);
    for (uint64_t sample = 0; sample <= samples; ++sample) {
        double start = now_ns();
        for (uint64_t iteration = 0; iteration < (sample ? iterations : 1); ++iteration) {
            for (uint64_t i = 0; i < count; ++i) {
                size_t size;
                char *result = temper_call(inputs[i], sizes[i], &size);
                if (!result) return 10;
                temper_free(result);
            }
        }
        if (sample) printf("%.3f\n", (now_ns() - start) / (double)iterations);
    }
    for (uint64_t i = 0; i < count; ++i) free(inputs[i]);
    free(inputs); free(sizes);
    return 0;
}
