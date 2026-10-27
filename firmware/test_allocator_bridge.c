#include "allocator_bridge.h"
#include <stdio.h>

#define CHECK(x) do { if (!(x)) { \
    fprintf(stderr, "bridge check failed at line %d: %s\n", __LINE__, #x); \
    return 1; \
} } while (0)
#define OK(x) CHECK((x) == OP1_ADAPTER_OK)

/* Native-only fake allocator. It never executes or calls target firmware. */
static unsigned char storage[OP1_ADAPTER_MAX_STAGING + 16u];
static op1_adapter *active;
static op1_allocator_bridge *active_bridge;
static uint32_t requested;
static void *freed;
static unsigned alloc_calls;
static unsigned free_calls;
static unsigned alternate_calls;
static int fail_allocation;
static int allocation_reentry;
static int release_reentry;
static int reentry_failed;

static void *single_allocate(uint32_t bytes)
{
    uint32_t unused = 0u;
    requested = bytes;
    ++alloc_calls;
    if (allocation_reentry != 0) {
        if (op1_adapter_bind_allocator_bridge(active, active_bridge) != OP1_ADAPTER_REENTRANT ||
            op1_adapter_bind_allocator_bridge(active, NULL) != OP1_ADAPTER_REENTRANT ||
            op1_adapter_begin_writer(active, &unused) != OP1_ADAPTER_REENTRANT ||
            op1_adapter_fault(active) != OP1_ADAPTER_REENTRANT) {
            reentry_failed = 1;
        }
    }
    return fail_allocation != 0 ? NULL : &storage[8];
}

static void single_release(void *raw)
{
    freed = raw;
    ++free_calls;
    if (release_reentry != 0) {
        if (op1_adapter_bind_allocator_bridge(active, active_bridge) != OP1_ADAPTER_REENTRANT ||
            op1_adapter_bind_allocator_bridge(active, NULL) != OP1_ADAPTER_REENTRANT ||
            op1_adapter_invalidate(active) != OP1_ADAPTER_REENTRANT) {
            reentry_failed = 1;
        }
    }
}

static void *alternate_allocate(uint32_t bytes)
{
    (void)bytes;
    ++alternate_calls;
    return &storage[0];
}

static void alternate_release(void *raw)
{
    (void)raw;
    ++alternate_calls;
}

static void reset_native_log(void)
{
    active = NULL;
    active_bridge = NULL;
    requested = 0u;
    freed = NULL;
    alloc_calls = 0u;
    free_calls = 0u;
    alternate_calls = 0u;
    fail_allocation = 0;
    allocation_reentry = 0;
    release_reentry = 0;
    reentry_failed = 0;
}

static int forwards_payload_and_exact_free(void)
{
    const uint32_t sizes[] = {1u, 2u, 257u, OP1_ADAPTER_MAX_STAGING};
    unsigned i;
    for (i = 0u; i < sizeof(sizes) / sizeof(sizes[0]); ++i) {
        op1_adapter adapter = OP1_ADAPTER_INITIALIZER;
        op1_allocator_bridge bridge = {single_allocate, single_release};
        uint32_t drain = 0u;
        reset_native_log();
        OK(op1_adapter_bind_allocator_bridge(&adapter, &bridge));
        CHECK(alloc_calls == 0u && free_calls == 0u);
        OK(op1_adapter_reserve(&adapter, sizes[i]));
        CHECK(requested == sizes[i] && alloc_calls == 1u);
        CHECK(adapter.raw == &storage[8] && adapter.capacity == sizes[i]);
        CHECK(op1_adapter_capture_allowed(&adapter) == 0);
        OK(op1_adapter_begin_drain(&adapter, &drain));
        OK(op1_adapter_release_after_drain(&adapter, drain));
        CHECK(freed == &storage[8] && free_calls == 1u && adapter.raw == NULL);
        CHECK(op1_adapter_release_after_drain(&adapter, drain) == OP1_ADAPTER_NOT_RESERVED);
        CHECK(free_calls == 1u);
    }
    return 0;
}

static int rejects_null_or_partial_context(void)
{
    op1_adapter adapter = OP1_ADAPTER_INITIALIZER;
    op1_allocator_bridge missing_alloc = {NULL, single_release};
    op1_allocator_bridge missing_free = {single_allocate, NULL};
    op1_allocator_bridge valid = {single_allocate, single_release};
    reset_native_log();
    CHECK(op1_adapter_bind_allocator_bridge(NULL, &valid) == OP1_ADAPTER_INVALID);
    CHECK(op1_adapter_bind_allocator_bridge(NULL, NULL) == OP1_ADAPTER_INVALID);
    CHECK(op1_adapter_bind_allocator_bridge(&adapter, NULL) == OP1_ADAPTER_INVALID);
    CHECK(op1_adapter_bind_allocator_bridge(&adapter, &missing_alloc) == OP1_ADAPTER_INVALID);
    CHECK(op1_adapter_bind_allocator_bridge(&adapter, &missing_free) == OP1_ADAPTER_INVALID);
    CHECK(adapter.bound == 0u && adapter.raw == NULL);
    CHECK(alloc_calls == 0u && free_calls == 0u);
    OK(op1_adapter_bind_allocator_bridge(&adapter, &valid));
    CHECK(op1_adapter_capture_allowed(&adapter) == 0);
    return 0;
}

static int rejects_bad_sizes_without_entering_bridge(void)
{
    op1_adapter adapter = OP1_ADAPTER_INITIALIZER;
    op1_allocator_bridge bridge = {single_allocate, single_release};
    reset_native_log();
    OK(op1_adapter_bind_allocator_bridge(&adapter, &bridge));
    CHECK(op1_adapter_reserve(&adapter, 0u) == OP1_ADAPTER_SIZE);
    CHECK(op1_adapter_reserve(&adapter, OP1_ADAPTER_MAX_STAGING + 1u) == OP1_ADAPTER_SIZE);
    CHECK(op1_adapter_reserve(&adapter, UINT32_MAX) == OP1_ADAPTER_SIZE);
    CHECK(alloc_calls == 0u && free_calls == 0u && adapter.raw == NULL);
    return 0;
}

static int allocation_failure_and_later_success(void)
{
    op1_adapter adapter = OP1_ADAPTER_INITIALIZER;
    op1_allocator_bridge bridge = {single_allocate, single_release};
    uint32_t drain = 0u;
    reset_native_log();
    OK(op1_adapter_bind_allocator_bridge(&adapter, &bridge));
    fail_allocation = 1;
    CHECK(op1_adapter_reserve(&adapter, 123u) == OP1_ADAPTER_NO_MEMORY);
    CHECK(requested == 123u && alloc_calls == 1u && adapter.raw == NULL);
    CHECK(adapter.capacity == 0u && free_calls == 0u);
    fail_allocation = 0;
    OK(op1_adapter_reserve(&adapter, 321u));
    CHECK(requested == 321u && alloc_calls == 2u);
    OK(op1_adapter_begin_drain(&adapter, &drain));
    OK(op1_adapter_release_after_drain(&adapter, drain));
    CHECK(free_calls == 1u && freed == &storage[8]);
    return 0;
}

static int immutable_binding_across_reservation_generations(void)
{
    op1_adapter adapter = OP1_ADAPTER_INITIALIZER;
    op1_allocator_bridge bridge = {single_allocate, single_release};
    op1_allocator_bridge replacement = {alternate_allocate, alternate_release};
    uint32_t old_drain = 0u;
    unsigned generation;
    reset_native_log();
    OK(op1_adapter_bind_allocator_bridge(&adapter, &bridge));
    for (generation = 0u; generation < 256u; ++generation) {
        uint32_t drain = 0u;
        CHECK(op1_adapter_bind_allocator_bridge(&adapter, &replacement) == OP1_ADAPTER_ALREADY_BOUND);
        CHECK(op1_adapter_bind_allocator_bridge(&adapter, NULL) == OP1_ADAPTER_INVALID);
        CHECK(adapter.allocator_context == &bridge);
        OK(op1_adapter_reserve(&adapter, generation + 1u));
        OK(op1_adapter_begin_drain(&adapter, &drain));
        CHECK(drain != old_drain);
        CHECK(op1_adapter_release_after_drain(&adapter, old_drain) == OP1_ADAPTER_WRONG_DRAIN);
        OK(op1_adapter_release_after_drain(&adapter, drain));
        old_drain = drain;
    }
    CHECK(alloc_calls == 256u && free_calls == 256u && alternate_calls == 0u);
    CHECK(op1_adapter_capture_allowed(&adapter) == 0);
    return 0;
}

static int composed_lease_drain_and_withdrawal(void)
{
    op1_adapter adapter = OP1_ADAPTER_INITIALIZER;
    op1_allocator_bridge bridge = {single_allocate, single_release};
    uint32_t writer = 0u, hold = 0u, drain = 0u;
    reset_native_log();
    OK(op1_adapter_bind_allocator_bridge(&adapter, &bridge));
    OK(op1_adapter_reserve(&adapter, 32u));
    OK(op1_adapter_begin_writer(&adapter, &writer));
    OK(op1_adapter_begin_hold(&adapter, &hold));
    OK(op1_adapter_invalidate(&adapter));
    OK(op1_adapter_begin_drain(&adapter, &drain));
    CHECK(op1_adapter_release_after_drain(&adapter, drain) == OP1_ADAPTER_BUSY);
    OK(op1_adapter_end_writer(&adapter, writer));
    CHECK(op1_adapter_release_after_drain(&adapter, drain) == OP1_ADAPTER_BUSY);
    OK(op1_adapter_end_hold(&adapter, hold));
    OK(op1_adapter_release_after_drain(&adapter, drain));
    CHECK(free_calls == 1u && freed == &storage[8]);
    CHECK(op1_adapter_reserve(&adapter, 32u) == OP1_ADAPTER_WITHDRAWN);
    return 0;
}

static int allocation_reentry_composes_fail_closed(void)
{
    int null_result;
    for (null_result = 0; null_result < 2; ++null_result) {
        op1_adapter adapter = OP1_ADAPTER_INITIALIZER;
        op1_allocator_bridge bridge = {single_allocate, single_release};
        uint32_t drain = 0u;
        reset_native_log();
        active = &adapter;
        active_bridge = &bridge;
        allocation_reentry = 1;
        fail_allocation = null_result;
        OK(op1_adapter_bind_allocator_bridge(&adapter, &bridge));
        CHECK(op1_adapter_reserve(&adapter, 64u) == OP1_ADAPTER_POISONED);
        CHECK(reentry_failed == 0 && alloc_calls == 1u && free_calls == 0u);
        CHECK(adapter.raw == (null_result != 0 ? NULL : &storage[8]));
        CHECK(adapter.capacity == (null_result != 0 ? 0u : 64u));
        CHECK(op1_adapter_begin_drain(&adapter, &drain) == OP1_ADAPTER_POISONED);
        CHECK(op1_adapter_capture_allowed(&adapter) == 0);
    }
    return 0;
}

static int release_reentry_records_one_committed_free(void)
{
    op1_adapter adapter = OP1_ADAPTER_INITIALIZER;
    op1_allocator_bridge bridge = {single_allocate, single_release};
    uint32_t drain = 0u;
    reset_native_log();
    active = &adapter;
    active_bridge = &bridge;
    OK(op1_adapter_bind_allocator_bridge(&adapter, &bridge));
    OK(op1_adapter_reserve(&adapter, 64u));
    OK(op1_adapter_begin_drain(&adapter, &drain));
    release_reentry = 1;
    CHECK(op1_adapter_release_after_drain(&adapter, drain) == OP1_ADAPTER_POISONED);
    CHECK(reentry_failed == 0 && free_calls == 1u && freed == &storage[8]);
    CHECK(adapter.raw == NULL && adapter.capacity == 0u);
    CHECK(op1_adapter_release_after_drain(&adapter, drain) == OP1_ADAPTER_POISONED);
    CHECK(free_calls == 1u && op1_adapter_capture_allowed(&adapter) == 0);
    return 0;
}

int main(void)
{
    if (forwards_payload_and_exact_free() || rejects_null_or_partial_context() ||
        rejects_bad_sizes_without_entering_bridge() || allocation_failure_and_later_success() ||
        immutable_binding_across_reservation_generations() || composed_lease_drain_and_withdrawal() ||
        allocation_reentry_composes_fail_closed() || release_reentry_records_one_committed_free()) {
        return 1;
    }
    puts("allocator bridge: 8 native scenario groups passed (including 256 generations)");
    return 0;
}
