#include <lean/lean.h>
#include <stddef.h>
#define PUBLIC __attribute__((visibility("default")))
extern lean_object *initialize_Entry(uint8_t builtin);
extern void lean_initialize_runtime_module(void);
typedef struct { const unsigned char *data; size_t len; } temper_bytes;
extern void *temper_rs_buffers_call(uint64_t, const temper_bytes *, size_t);
extern int temper_rs_buffers_status(const void *);
extern size_t temper_rs_buffers_count(const void *);
extern const unsigned char *temper_rs_buffers_view(const void *, size_t, size_t *);
extern void temper_rs_buffers_free(void *);
PUBLIC int temper_initialize(void) {
    lean_initialize_runtime_module();
    lean_object *result = initialize_Entry(1);
    int ok = lean_io_result_is_ok(result);
    lean_dec(result);
    lean_io_mark_end_initialization();
    return ok ? 0 : 1;
}
PUBLIC void *temper_buffers_call(uint64_t p, const temper_bytes *b, size_t n) { return temper_rs_buffers_call(p,b,n); }
PUBLIC int temper_buffers_status(const void *r) { return temper_rs_buffers_status(r); }
PUBLIC size_t temper_buffers_count(const void *r) { return temper_rs_buffers_count(r); }
PUBLIC const unsigned char *temper_buffers_view(const void *r, size_t i, size_t *n) { return temper_rs_buffers_view(r,i,n); }
PUBLIC void temper_buffers_free(void *r) { temper_rs_buffers_free(r); }
