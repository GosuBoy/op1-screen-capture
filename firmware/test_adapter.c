#include "adapter.h"
#include <stdio.h>

#define CHECK(x) do { if (!(x)) { \
    fprintf(stderr, "check failed at line %d: %s\n", __LINE__, #x); return 1; \
} } while (0)
#define OK(x) CHECK((x) == OP1_ADAPTER_OK)

static unsigned char arena[OP1_ADAPTER_MAX_STAGING + 16u];
typedef struct {
    op1_adapter *state;
    unsigned allocs;
    unsigned frees;
    uint32_t requested;
    void *freed;
    int fail;
    int reenter;
    int reentry_failed;
} mock;

static void *allocate(void *context, uint32_t bytes);
static void release(void *context, void *raw);

static void try_reentry(mock *m)
{
    uint32_t out = 0u;
    op1_adapter *s = m->state;
    if (op1_adapter_reserve(s, 2u) != OP1_ADAPTER_REENTRANT ||
        op1_adapter_bind(s, allocate, release, m) != OP1_ADAPTER_REENTRANT ||
        op1_adapter_bind(s, NULL, release, m) != OP1_ADAPTER_REENTRANT ||
        op1_adapter_begin_writer(s, &out) != OP1_ADAPTER_REENTRANT ||
        op1_adapter_end_writer(s, 1u) != OP1_ADAPTER_REENTRANT ||
        op1_adapter_begin_hold(s, &out) != OP1_ADAPTER_REENTRANT ||
        op1_adapter_end_hold(s, 1u) != OP1_ADAPTER_REENTRANT ||
        op1_adapter_begin_drain(s, &out) != OP1_ADAPTER_REENTRANT ||
        op1_adapter_release_after_drain(s, s->drain_id) != OP1_ADAPTER_REENTRANT ||
        op1_adapter_invalidate(s) != OP1_ADAPTER_REENTRANT ||
        op1_adapter_fault(s) != OP1_ADAPTER_REENTRANT ||
        op1_adapter_capture_allowed(s) != 0)
        m->reentry_failed = 1;
}

static void *allocate(void *context, uint32_t bytes)
{
    mock *m = context;
    ++m->allocs;
    m->requested = bytes;
    if (m->reenter) try_reentry(m);
    return m->fail ? NULL : (void *)(arena + 8u);
}

static void release(void *context, void *raw)
{
    mock *m = context;
    ++m->frees;
    m->freed = raw;
    if (m->reenter) try_reentry(m);
}

static int disabled_and_binding(void)
{
    op1_adapter s = OP1_ADAPTER_INITIALIZER;
    mock m = {0};
    m.state = &s;
    CHECK(op1_adapter_capture_allowed(NULL) == 0);
    CHECK(op1_adapter_capture_allowed(&s) == 0);
    CHECK(op1_adapter_reserve(&s, 2u) == OP1_ADAPTER_UNBOUND);
    CHECK(op1_adapter_bind(NULL, allocate, release, &m) == OP1_ADAPTER_INVALID);
    CHECK(op1_adapter_bind(&s, NULL, release, &m) == OP1_ADAPTER_INVALID);
    CHECK(op1_adapter_bind(&s, allocate, NULL, &m) == OP1_ADAPTER_INVALID);
    OK(op1_adapter_bind(&s, allocate, release, &m));
    CHECK(op1_adapter_bind(&s, allocate, release, NULL) == OP1_ADAPTER_ALREADY_BOUND);
    CHECK(s.allocator_context == &m);
    CHECK(op1_adapter_capture_allowed(&s) == 0);
    return 0;
}

static int bounded_allocation_failure_and_exact_free(void)
{
    op1_adapter s = OP1_ADAPTER_INITIALIZER;
    mock m = {0};
    uint32_t drain;
    m.state = &s;
    OK(op1_adapter_bind(&s, allocate, release, &m));
    CHECK(op1_adapter_reserve(&s, 0u) == OP1_ADAPTER_SIZE);
    CHECK(op1_adapter_reserve(&s, OP1_ADAPTER_MAX_STAGING + 1u) == OP1_ADAPTER_SIZE);
    CHECK(op1_adapter_reserve(&s, UINT32_MAX) == OP1_ADAPTER_SIZE);
    CHECK(m.allocs == 0u);
    m.fail = 1;
    CHECK(op1_adapter_reserve(&s, 2u) == OP1_ADAPTER_NO_MEMORY);
    CHECK(m.allocs == 1u && s.raw == NULL && s.capacity == 0u);
    m.fail = 0;
    OK(op1_adapter_reserve(&s, OP1_ADAPTER_MAX_STAGING));
    CHECK(m.allocs == 2u && m.requested == OP1_ADAPTER_MAX_STAGING);
    CHECK(s.raw == arena + 8u && s.capacity == OP1_ADAPTER_MAX_STAGING);
    CHECK(op1_adapter_reserve(&s, 2u) == OP1_ADAPTER_BUSY);
    CHECK(op1_adapter_release_after_drain(&s, 0u) == OP1_ADAPTER_WRONG_DRAIN);
    OK(op1_adapter_begin_drain(&s, &drain));
    OK(op1_adapter_release_after_drain(&s, drain));
    CHECK(m.frees == 1u && m.freed == arena + 8u);
    CHECK(s.raw == NULL && s.capacity == 0u);
    CHECK(op1_adapter_release_after_drain(&s, drain) == OP1_ADAPTER_NOT_RESERVED);
    CHECK(m.frees == 1u && op1_adapter_capture_allowed(&s) == 0);
    return 0;
}

static int nested_owners_and_drain(void)
{
    op1_adapter s = OP1_ADAPTER_INITIALIZER;
    mock m = {0};
    uint32_t a, b, hold, drain, later, unused;
    m.state = &s;
    OK(op1_adapter_bind(&s, allocate, release, &m));
    OK(op1_adapter_begin_writer(&s, &a));
    CHECK(op1_adapter_reserve(&s, 32u) == OP1_ADAPTER_BUSY);
    OK(op1_adapter_end_writer(&s, a));
    OK(op1_adapter_reserve(&s, 32u));
    OK(op1_adapter_begin_writer(&s, &a));
    OK(op1_adapter_begin_writer(&s, &b));
    CHECK(a != b);
    OK(op1_adapter_begin_hold(&s, &hold));
    OK(op1_adapter_begin_drain(&s, &drain));
    CHECK(op1_adapter_begin_drain(&s, &unused) == OP1_ADAPTER_BUSY);
    CHECK(op1_adapter_begin_hold(&s, &unused) == OP1_ADAPTER_WITHDRAWN);
    CHECK(op1_adapter_release_after_drain(&s, drain) == OP1_ADAPTER_BUSY);
    OK(op1_adapter_end_writer(&s, b));
    OK(op1_adapter_end_writer(&s, a));
    CHECK(op1_adapter_release_after_drain(&s, drain) == OP1_ADAPTER_BUSY);
    OK(op1_adapter_begin_writer(&s, &later));
    OK(op1_adapter_end_hold(&s, hold));
    CHECK(op1_adapter_release_after_drain(&s, drain) == OP1_ADAPTER_BUSY);
    OK(op1_adapter_end_writer(&s, later));
    OK(op1_adapter_release_after_drain(&s, drain));
    CHECK(m.frees == 1u);
    return 0;
}

static int lifecycle_retains_holds(void)
{
    op1_adapter s = OP1_ADAPTER_INITIALIZER;
    mock m = {0};
    uint32_t hold, drain, ignored;
    m.state = &s;
    OK(op1_adapter_bind(&s, allocate, release, &m));
    OK(op1_adapter_reserve(&s, 32u));
    OK(op1_adapter_begin_hold(&s, &hold));
    OK(op1_adapter_invalidate(&s));
    CHECK(s.raw == arena + 8u && m.frees == 0u);
    CHECK(op1_adapter_begin_hold(&s, &ignored) == OP1_ADAPTER_WITHDRAWN);
    OK(op1_adapter_begin_drain(&s, &drain));
    CHECK(op1_adapter_release_after_drain(&s, drain) == OP1_ADAPTER_BUSY);
    OK(op1_adapter_end_hold(&s, hold));
    OK(op1_adapter_release_after_drain(&s, drain));
    CHECK(op1_adapter_reserve(&s, 32u) == OP1_ADAPTER_WITHDRAWN);
    CHECK(op1_adapter_bind(&s, allocate, release, &m) == OP1_ADAPTER_ALREADY_BOUND);
    return 0;
}

static int fault_never_recycles(void)
{
    op1_adapter s = OP1_ADAPTER_INITIALIZER;
    mock m = {0};
    uint32_t hold, drain;
    m.state = &s;
    OK(op1_adapter_bind(&s, allocate, release, &m));
    OK(op1_adapter_reserve(&s, 32u));
    OK(op1_adapter_begin_hold(&s, &hold));
    OK(op1_adapter_begin_drain(&s, &drain));
    OK(op1_adapter_fault(&s));
    OK(op1_adapter_end_hold(&s, hold));
    CHECK(op1_adapter_release_after_drain(&s, drain) == OP1_ADAPTER_POISONED);
    CHECK(s.raw == arena + 8u && m.frees == 0u);
    CHECK(op1_adapter_capture_allowed(&s) == 0);
    return 0;
}

static int stale_drain_cannot_release_replacement(void)
{
    op1_adapter s = OP1_ADAPTER_INITIALIZER;
    mock m = {0};
    uint32_t old, current;
    m.state = &s;
    OK(op1_adapter_bind(&s, allocate, release, &m));
    OK(op1_adapter_reserve(&s, 32u));
    OK(op1_adapter_begin_drain(&s, &old));
    OK(op1_adapter_release_after_drain(&s, old));
    OK(op1_adapter_reserve(&s, 64u));
    OK(op1_adapter_begin_drain(&s, &current));
    CHECK(old != current);
    CHECK(op1_adapter_release_after_drain(&s, old) == OP1_ADAPTER_WRONG_DRAIN);
    CHECK(s.capacity == 64u && m.frees == 1u);
    OK(op1_adapter_release_after_drain(&s, current));
    CHECK(m.frees == 2u);
    return 0;
}

static int duplicate_lease_poison_retains_other_owner(void)
{
    op1_adapter s = OP1_ADAPTER_INITIALIZER;
    mock m = {0};
    uint32_t a, b, drain;
    m.state = &s;
    OK(op1_adapter_bind(&s, allocate, release, &m));
    OK(op1_adapter_reserve(&s, 32u));
    OK(op1_adapter_begin_writer(&s, &a));
    OK(op1_adapter_begin_writer(&s, &b));
    OK(op1_adapter_begin_drain(&s, &drain));
    OK(op1_adapter_end_writer(&s, a));
    CHECK(op1_adapter_end_writer(&s, a) == OP1_ADAPTER_UNKNOWN_LEASE);
    CHECK(s.writers[1] == b);
    OK(op1_adapter_end_writer(&s, b));
    CHECK(op1_adapter_release_after_drain(&s, drain) == OP1_ADAPTER_POISONED);
    CHECK(m.frees == 0u && s.raw != NULL);
    return 0;
}

static int capacity_overflow_is_permanent(void)
{
    unsigned kind, i;
    for (kind = 0u; kind < 2u; ++kind) {
        op1_adapter s = OP1_ADAPTER_INITIALIZER;
        mock m = {0};
        uint32_t ids[OP1_ADAPTER_MAX_WRITERS], drain, ignored;
        unsigned count = kind == 0u ? OP1_ADAPTER_MAX_WRITERS : OP1_ADAPTER_MAX_HOLDS;
        m.state = &s;
        OK(op1_adapter_bind(&s, allocate, release, &m));
        OK(op1_adapter_reserve(&s, 32u));
        for (i = 0u; i < count; ++i) {
            if (kind == 0u) OK(op1_adapter_begin_writer(&s, &ids[i]));
            else OK(op1_adapter_begin_hold(&s, &ids[i]));
        }
        if (kind == 0u) CHECK(op1_adapter_begin_writer(&s, &ignored) == OP1_ADAPTER_CAPACITY);
        else CHECK(op1_adapter_begin_hold(&s, &ignored) == OP1_ADAPTER_CAPACITY);
        for (i = 0u; i < count; ++i) {
            if (kind == 0u) OK(op1_adapter_end_writer(&s, ids[i]));
            else OK(op1_adapter_end_hold(&s, ids[i]));
        }
        CHECK(op1_adapter_begin_drain(&s, &drain) == OP1_ADAPTER_POISONED);
        CHECK(s.raw != NULL && m.frees == 0u);
    }
    return 0;
}

static int ids_never_wrap(void)
{
    op1_adapter s = OP1_ADAPTER_INITIALIZER;
    mock m = {0};
    uint32_t last, ignored;
    m.state = &s;
    OK(op1_adapter_bind(&s, allocate, release, &m));
    OK(op1_adapter_reserve(&s, 32u));
    /* Test-only boundary injection, forbidden for integration callers. */
    s.last_id = UINT32_MAX - 1u;
    OK(op1_adapter_begin_writer(&s, &last));
    CHECK(last == UINT32_MAX);
    OK(op1_adapter_end_writer(&s, last));
    CHECK(op1_adapter_begin_hold(&s, &ignored) == OP1_ADAPTER_ID_EXHAUSTED);
    OK(op1_adapter_invalidate(&s));
    CHECK(op1_adapter_begin_writer(&s, &ignored) == OP1_ADAPTER_POISONED);
    CHECK(s.last_id == UINT32_MAX && s.raw != NULL && m.frees == 0u);
    return 0;
}

static int allocation_reentry_poison_retains_pointer(void)
{
    int fail;
    for (fail = 0; fail < 2; ++fail) {
        op1_adapter s = OP1_ADAPTER_INITIALIZER;
        mock m = {0};
        uint32_t drain;
        m.state = &s;
        m.reenter = 1;
        m.fail = fail;
        OK(op1_adapter_bind(&s, allocate, release, &m));
        CHECK(op1_adapter_reserve(&s, 32u) == OP1_ADAPTER_POISONED);
        CHECK(m.allocs == 1u && !m.reentry_failed);
        CHECK(s.raw == (fail ? NULL : (void *)(arena + 8u)));
        CHECK(s.withdrawn && s.poisoned && !s.in_callback);
        CHECK(op1_adapter_begin_drain(&s, &drain) == OP1_ADAPTER_POISONED);
        CHECK(m.frees == 0u);
        CHECK(op1_adapter_capture_allowed(&s) == 0);
    }
    return 0;
}

static int free_reentry_reports_poison_without_double_free(void)
{
    op1_adapter s = OP1_ADAPTER_INITIALIZER;
    mock m = {0};
    uint32_t drain;
    m.state = &s;
    OK(op1_adapter_bind(&s, allocate, release, &m));
    OK(op1_adapter_reserve(&s, 32u));
    OK(op1_adapter_begin_drain(&s, &drain));
    m.reenter = 1;
    CHECK(op1_adapter_release_after_drain(&s, drain) == OP1_ADAPTER_POISONED);
    CHECK(m.frees == 1u && m.freed == arena + 8u && !m.reentry_failed);
    CHECK(s.raw == NULL && !s.in_callback && s.poisoned);
    CHECK(op1_adapter_release_after_drain(&s, drain) == OP1_ADAPTER_POISONED);
    CHECK(op1_adapter_reserve(&s, 32u) == OP1_ADAPTER_POISONED);
    CHECK(m.frees == 1u && m.allocs == 1u);
    CHECK(op1_adapter_capture_allowed(&s) == 0);
    return 0;
}

static int invalid_lease_and_unreserved_hold(void)
{
    op1_adapter s = OP1_ADAPTER_INITIALIZER;
    mock m = {0};
    uint32_t ignored;
    m.state = &s;
    OK(op1_adapter_bind(&s, allocate, release, &m));
    CHECK(op1_adapter_begin_hold(&s, &ignored) == OP1_ADAPTER_NOT_RESERVED);
    CHECK(op1_adapter_begin_drain(&s, &ignored) == OP1_ADAPTER_NOT_RESERVED);
    CHECK(op1_adapter_end_hold(&s, 0u) == OP1_ADAPTER_UNKNOWN_LEASE);
    CHECK(op1_adapter_reserve(&s, 32u) == OP1_ADAPTER_POISONED);
    CHECK(m.allocs == 0u);
    return 0;
}

int main(void)
{
    int (*const tests[])(void) = {
        disabled_and_binding, bounded_allocation_failure_and_exact_free,
        nested_owners_and_drain, lifecycle_retains_holds, fault_never_recycles,
        stale_drain_cannot_release_replacement,
        duplicate_lease_poison_retains_other_owner, capacity_overflow_is_permanent,
        ids_never_wrap, allocation_reentry_poison_retains_pointer,
        free_reentry_reports_poison_without_double_free,
        invalid_lease_and_unreserved_hold
    };
    unsigned i;
    for (i = 0u; i < sizeof(tests) / sizeof(tests[0]); ++i) {
        if (tests[i]() != 0) return 1;
    }
    printf("disabled adapter: %u native tests passed\n", i);
    return 0;
}
