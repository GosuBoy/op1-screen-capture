#include "allocator_bridge.h"
#include <assert.h>
#include <stdio.h>

#ifdef NDEBUG
#error "Independent bridge checks require assertions"
#endif

/* Native-only fixtures. No target functions or addresses are called. */
static unsigned char arena_a[OP1_ADAPTER_MAX_STAGING + 16u];
static unsigned char arena_b[OP1_ADAPTER_MAX_STAGING + 16u];
static unsigned alloc_a, free_a, alloc_b, free_b;
static uint32_t size_a, size_b;
static void *last_a, *last_b;
static int fail_a, reenter_alloc, reenter_free;
static op1_adapter *during_callback;
static op1_allocator_bridge *rebind_context;
static op1_adapter_result reentry_result;

static void *allocate_a(uint32_t bytes)
{
    ++alloc_a;
    size_a = bytes;
    if (reenter_alloc) {
        reentry_result = op1_adapter_bind_allocator_bridge(during_callback, rebind_context);
    }
    return fail_a ? NULL : arena_a + 8;
}

static void release_a(void *raw)
{
    ++free_a;
    last_a = raw;
    if (reenter_free) {
        reentry_result = op1_adapter_bind_allocator_bridge(during_callback, rebind_context);
    }
}

static void *allocate_b(uint32_t bytes)
{
    ++alloc_b;
    size_b = bytes;
    return arena_b + 8;
}

static void release_b(void *raw)
{
    ++free_b;
    last_b = raw;
}

static void reset_log(void)
{
    alloc_a = free_a = alloc_b = free_b = 0;
    size_a = size_b = 0;
    last_a = last_b = NULL;
    fail_a = reenter_alloc = reenter_free = 0;
    during_callback = NULL;
    rebind_context = NULL;
    reentry_result = OP1_ADAPTER_OK;
}

static void release_reserved(op1_adapter *adapter)
{
    uint32_t drain;
    assert(op1_adapter_begin_drain(adapter, &drain) == OP1_ADAPTER_OK);
    assert(op1_adapter_release_after_drain(adapter, drain) == OP1_ADAPTER_OK);
}

static void independent_contexts_and_payloads(void)
{
    op1_adapter a = OP1_ADAPTER_INITIALIZER, b = OP1_ADAPTER_INITIALIZER;
    op1_allocator_bridge ca = {allocate_a, release_a}, cb = {allocate_b, release_b};
    reset_log();
    assert(op1_adapter_bind_allocator_bridge(&a, &ca) == OP1_ADAPTER_OK);
    assert(op1_adapter_bind_allocator_bridge(&b, &cb) == OP1_ADAPTER_OK);
    assert(op1_adapter_reserve(&a, 257) == OP1_ADAPTER_OK);
    assert(op1_adapter_reserve(&b, 65537) == OP1_ADAPTER_OK);
    assert(size_a == 257 && size_b == 65537 && alloc_a == 1 && alloc_b == 1);
    assert(a.raw == arena_a + 8 && b.raw == arena_b + 8);
    assert(op1_adapter_bind_allocator_bridge(&a, &cb) == OP1_ADAPTER_ALREADY_BOUND);
    assert(a.allocator_context == &ca && b.allocator_context == &cb);
    release_reserved(&b);
    release_reserved(&a);
    assert(free_a == 1 && free_b == 1 && last_a == arena_a + 8 && last_b == arena_b + 8);
    assert(!op1_adapter_capture_allowed(&a) && !op1_adapter_capture_allowed(&b));
}

static void repeated_reservations_preserve_binding(void)
{
    op1_adapter a = OP1_ADAPTER_INITIALIZER;
    op1_allocator_bridge ca = {allocate_a, release_a}, cb = {allocate_b, release_b};
    uint32_t old = 0;
    unsigned i;
    reset_log();
    assert(op1_adapter_bind_allocator_bridge(&a, &ca) == OP1_ADAPTER_OK);
    for (i = 0; i < 512; ++i) {
        uint32_t drain;
        assert(op1_adapter_reserve(&a, i + 1) == OP1_ADAPTER_OK);
        assert(size_a == i + 1);
        assert(op1_adapter_begin_drain(&a, &drain) == OP1_ADAPTER_OK);
        assert(drain != old);
        assert(op1_adapter_release_after_drain(&a, old) == OP1_ADAPTER_WRONG_DRAIN);
        assert(op1_adapter_bind_allocator_bridge(&a, &cb) == OP1_ADAPTER_ALREADY_BOUND);
        assert(op1_adapter_release_after_drain(&a, drain) == OP1_ADAPTER_OK);
        assert(a.allocator_context == &ca);
        old = drain;
    }
    assert(alloc_a == 512 && free_a == 512 && alloc_b == 0 && free_b == 0);
}

static void release_permutation(const unsigned order[6])
{
    op1_adapter a = OP1_ADAPTER_INITIALIZER;
    op1_allocator_bridge ca = {allocate_a, release_a};
    uint32_t leases[6], drain;
    unsigned i;
    reset_log();
    assert(op1_adapter_bind_allocator_bridge(&a, &ca) == OP1_ADAPTER_OK);
    assert(op1_adapter_reserve(&a, 17) == OP1_ADAPTER_OK);
    for (i = 0; i < 3; ++i) {
        assert(op1_adapter_begin_writer(&a, &leases[i]) == OP1_ADAPTER_OK);
        assert(op1_adapter_begin_hold(&a, &leases[i + 3]) == OP1_ADAPTER_OK);
    }
    assert(op1_adapter_begin_drain(&a, &drain) == OP1_ADAPTER_OK);
    for (i = 0; i < 6; ++i) {
        unsigned index = order[i];
        assert(op1_adapter_release_after_drain(&a, drain) == OP1_ADAPTER_BUSY);
        assert(free_a == 0 && a.raw == arena_a + 8);
        assert((index < 3 ? op1_adapter_end_writer(&a, leases[index]) :
                op1_adapter_end_hold(&a, leases[index])) == OP1_ADAPTER_OK);
    }
    assert(op1_adapter_release_after_drain(&a, drain) == OP1_ADAPTER_OK);
    assert(free_a == 1 && last_a == arena_a + 8);
}

static unsigned permutations(unsigned order[6], unsigned at)
{
    unsigned i, count = 0;
    if (at == 6) { release_permutation(order); return 1; }
    for (i = at; i < 6; ++i) {
        unsigned swap = order[at]; order[at] = order[i]; order[i] = swap;
        count += permutations(order, at + 1);
        swap = order[at]; order[at] = order[i]; order[i] = swap;
    }
    return count;
}

static void invalid_context_and_size_do_not_call_allocator(void)
{
    op1_adapter a = OP1_ADAPTER_INITIALIZER;
    op1_allocator_bridge ca = {allocate_a, release_a};
    op1_allocator_bridge bad_a = {NULL, release_a}, bad_b = {allocate_a, NULL};
    reset_log();
    assert(op1_adapter_bind_allocator_bridge(&a, NULL) == OP1_ADAPTER_INVALID);
    assert(op1_adapter_bind_allocator_bridge(&a, &bad_a) == OP1_ADAPTER_INVALID);
    assert(op1_adapter_bind_allocator_bridge(&a, &bad_b) == OP1_ADAPTER_INVALID);
    assert(!a.bound && !a.raw && alloc_a == 0 && free_a == 0);
    assert(op1_adapter_bind_allocator_bridge(&a, &ca) == OP1_ADAPTER_OK);
    assert(op1_adapter_reserve(&a, 0) == OP1_ADAPTER_SIZE);
    assert(op1_adapter_reserve(&a, UINT32_MAX) == OP1_ADAPTER_SIZE);
    assert(op1_adapter_reserve(&a, OP1_ADAPTER_MAX_STAGING + 1) == OP1_ADAPTER_SIZE);
    assert(alloc_a == 0);
    fail_a = 1;
    assert(op1_adapter_reserve(&a, 19) == OP1_ADAPTER_NO_MEMORY);
    assert(!a.raw && !a.capacity && free_a == 0);
    fail_a = 0;
    assert(op1_adapter_reserve(&a, OP1_ADAPTER_MAX_STAGING) == OP1_ADAPTER_OK);
    assert(size_a == OP1_ADAPTER_MAX_STAGING);
    release_reserved(&a);
}

static void every_rebind_shape_poisons_allocation_reentry(void)
{
    unsigned shape, returns_null;
    for (shape = 0; shape < 4; ++shape) for (returns_null = 0; returns_null < 2; ++returns_null) {
        op1_adapter a = OP1_ADAPTER_INITIALIZER;
        op1_allocator_bridge ca = {allocate_a, release_a};
        op1_allocator_bridge missing_a = {NULL, release_a}, missing_f = {allocate_a, NULL};
        op1_allocator_bridge *shapes[] = {NULL, &ca, &missing_a, &missing_f};
        uint32_t drain = 0;
        reset_log();
        during_callback = &a;
        rebind_context = shapes[shape];
        reenter_alloc = 1;
        fail_a = (int)returns_null;
        assert(op1_adapter_bind_allocator_bridge(&a, &ca) == OP1_ADAPTER_OK);
        assert(op1_adapter_reserve(&a, 31) == OP1_ADAPTER_POISONED);
        assert(reentry_result == OP1_ADAPTER_REENTRANT && a.poisoned && a.withdrawn);
        assert(a.raw == (returns_null ? NULL : arena_a + 8));
        assert(a.capacity == (returns_null ? 0 : 31));
        assert(op1_adapter_begin_drain(&a, &drain) == OP1_ADAPTER_POISONED);
        assert(free_a == 0 && !op1_adapter_capture_allowed(&a));
    }
}

static void every_rebind_shape_records_free_exactly_once(void)
{
    unsigned shape;
    for (shape = 0; shape < 4; ++shape) {
        op1_adapter a = OP1_ADAPTER_INITIALIZER;
        op1_allocator_bridge ca = {allocate_a, release_a};
        op1_allocator_bridge missing_a = {NULL, release_a}, missing_f = {allocate_a, NULL};
        op1_allocator_bridge *shapes[] = {NULL, &ca, &missing_a, &missing_f};
        uint32_t drain;
        reset_log();
        during_callback = &a;
        rebind_context = shapes[shape];
        assert(op1_adapter_bind_allocator_bridge(&a, &ca) == OP1_ADAPTER_OK);
        assert(op1_adapter_reserve(&a, 31) == OP1_ADAPTER_OK);
        assert(op1_adapter_begin_drain(&a, &drain) == OP1_ADAPTER_OK);
        reenter_free = 1;
        assert(op1_adapter_release_after_drain(&a, drain) == OP1_ADAPTER_POISONED);
        assert(reentry_result == OP1_ADAPTER_REENTRANT && a.poisoned);
        assert(!a.raw && !a.capacity && free_a == 1 && last_a == arena_a + 8);
        assert(op1_adapter_release_after_drain(&a, drain) == OP1_ADAPTER_POISONED);
        assert(free_a == 1 && !op1_adapter_capture_allowed(&a));
    }
}

int main(void)
{
    unsigned order[] = {0, 1, 2, 3, 4, 5};
    independent_contexts_and_payloads();
    repeated_reservations_preserve_binding();
    assert(permutations(order, 0) == 720);
    invalid_context_and_size_do_not_call_allocator();
    every_rebind_shape_poisons_allocation_reentry();
    every_rebind_shape_records_free_exactly_once();
    puts("Independent bridge checks passed: 6 groups, 720 drain orders, 512 generations");
    return 0;
}
