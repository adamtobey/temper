#include <lean/lean.h>
#include <stdlib.h>
#include <string.h>
#define TEMPER_EXPORT __attribute__((visibility("default")))

extern lean_object *initialize_Entry(uint8_t builtin);
extern lean_object *temper_lean_invoke(lean_object *request);
extern void lean_initialize_runtime_module(void);
extern void lean_initialize_thread(void);
extern void lean_finalize_thread(void);

TEMPER_EXPORT int temper_initialize(void) {
    lean_initialize_runtime_module();
    lean_object *result = initialize_Entry(1);
    int ok = lean_io_result_is_ok(result);
    lean_dec(result);
    lean_io_mark_end_initialization();
    return ok ? 0 : 1;
}

TEMPER_EXPORT char *temper_call(const char *input, size_t input_size, size_t *output_size) {
    lean_initialize_thread();
    lean_object *request = lean_mk_string_from_bytes(input, input_size);
    lean_object *response = temper_lean_invoke(request);
    *output_size = lean_string_size(response) - 1;
    char *out = malloc(*output_size + 1);
    if (out) memcpy(out, lean_string_cstr(response), *output_size + 1);
    lean_dec(response);
    lean_finalize_thread();
    return out;
}

TEMPER_EXPORT void temper_free(char *output) { free(output); }
