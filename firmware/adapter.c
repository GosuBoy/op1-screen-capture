#include "adapter.h"

static void poison(op1_adapter *s)
{
    s->poisoned = 1u;
    s->withdrawn = 1u;
}

static op1_adapter_result enter(op1_adapter *s)
{
    if (s == NULL) return OP1_ADAPTER_INVALID;
    if (s->in_callback != 0u) {
        poison(s);
        return OP1_ADAPTER_REENTRANT;
    }
    if (s->bound == 0u) return OP1_ADAPTER_UNBOUND;
    return OP1_ADAPTER_OK;
}

static int any(const uint32_t *leases, unsigned count)
{
    unsigned i;
    for (i = 0u; i < count; ++i) if (leases[i] != 0u) return 1;
    return 0;
}

static op1_adapter_result next_id(op1_adapter *s, uint32_t *id)
{
    if (s->last_id == UINT32_MAX) {
        poison(s);
        return OP1_ADAPTER_ID_EXHAUSTED;
    }
    s->last_id += 1u;
    *id = s->last_id;
    return OP1_ADAPTER_OK;
}

static op1_adapter_result acquire(op1_adapter *s, uint32_t *slots,
                                  unsigned count, uint32_t *out)
{
    unsigned i;
    op1_adapter_result result;
    if (out == NULL) {
        poison(s);
        return OP1_ADAPTER_INVALID;
    }
    for (i = 0u; i < count; ++i) {
        if (slots[i] == 0u) {
            result = next_id(s, &slots[i]);
            if (result == OP1_ADAPTER_OK) *out = slots[i];
            return result;
        }
    }
    poison(s);
    return OP1_ADAPTER_CAPACITY;
}

static op1_adapter_result end(op1_adapter *s, uint32_t *slots,
                              unsigned count, uint32_t lease)
{
    unsigned i;
    if (lease != 0u) {
        for (i = 0u; i < count; ++i) {
            if (slots[i] == lease) {
                slots[i] = 0u;
                return OP1_ADAPTER_OK;
            }
        }
    }
    poison(s);
    return OP1_ADAPTER_UNKNOWN_LEASE;
}

op1_adapter_result op1_adapter_bind(op1_adapter *s, op1_adapter_alloc_fn alloc,
                                    op1_adapter_free_fn free_fn, void *context)
{
    if (s == NULL) return OP1_ADAPTER_INVALID;
    if (s->in_callback != 0u) {
        poison(s);
        return OP1_ADAPTER_REENTRANT;
    }
    if (alloc == NULL || free_fn == NULL) return OP1_ADAPTER_INVALID;
    if (s->bound != 0u) return OP1_ADAPTER_ALREADY_BOUND;
    s->allocate = alloc;
    s->release = free_fn;
    s->allocator_context = context;
    s->bound = 1u;
    return OP1_ADAPTER_OK;
}

int op1_adapter_capture_allowed(const op1_adapter *s)
{
    (void)s;
    return 0;
}

op1_adapter_result op1_adapter_reserve(op1_adapter *s, uint32_t bytes)
{
    void *raw;
    op1_adapter_result result = enter(s);
    if (result != OP1_ADAPTER_OK) return result;
    if (s->poisoned != 0u) return OP1_ADAPTER_POISONED;
    if (s->withdrawn != 0u) return OP1_ADAPTER_WITHDRAWN;
    if (bytes == 0u || bytes > OP1_ADAPTER_MAX_STAGING) return OP1_ADAPTER_SIZE;
    if (s->raw != NULL || s->drain_id != 0u ||
        any(s->writers, OP1_ADAPTER_MAX_WRITERS) ||
        any(s->holds, OP1_ADAPTER_MAX_HOLDS)) return OP1_ADAPTER_BUSY;
    s->in_callback = 1u;
    raw = s->allocate(s->allocator_context, bytes);
    if (raw != NULL) {
        s->raw = raw;
        s->capacity = bytes;
    }
    s->in_callback = 0u;
    if (s->poisoned != 0u) return OP1_ADAPTER_POISONED;
    return raw == NULL ? OP1_ADAPTER_NO_MEMORY : OP1_ADAPTER_OK;
}

op1_adapter_result op1_adapter_begin_writer(op1_adapter *s, uint32_t *lease)
{
    op1_adapter_result result = enter(s);
    if (result != OP1_ADAPTER_OK) return result;
    if (s->poisoned != 0u) return OP1_ADAPTER_POISONED;
    return acquire(s, s->writers, OP1_ADAPTER_MAX_WRITERS, lease);
}

op1_adapter_result op1_adapter_end_writer(op1_adapter *s, uint32_t lease)
{
    op1_adapter_result result = enter(s);
    if (result != OP1_ADAPTER_OK) return result;
    return end(s, s->writers, OP1_ADAPTER_MAX_WRITERS, lease);
}

op1_adapter_result op1_adapter_begin_hold(op1_adapter *s, uint32_t *lease)
{
    op1_adapter_result result = enter(s);
    if (result != OP1_ADAPTER_OK) return result;
    if (s->poisoned != 0u) return OP1_ADAPTER_POISONED;
    if (s->raw == NULL) return OP1_ADAPTER_NOT_RESERVED;
    if (s->withdrawn != 0u || s->drain_id != 0u) return OP1_ADAPTER_WITHDRAWN;
    return acquire(s, s->holds, OP1_ADAPTER_MAX_HOLDS, lease);
}

op1_adapter_result op1_adapter_end_hold(op1_adapter *s, uint32_t lease)
{
    op1_adapter_result result = enter(s);
    if (result != OP1_ADAPTER_OK) return result;
    return end(s, s->holds, OP1_ADAPTER_MAX_HOLDS, lease);
}

op1_adapter_result op1_adapter_invalidate(op1_adapter *s)
{
    op1_adapter_result result = enter(s);
    if (result != OP1_ADAPTER_OK) return result;
    s->withdrawn = 1u;
    return OP1_ADAPTER_OK;
}

op1_adapter_result op1_adapter_fault(op1_adapter *s)
{
    op1_adapter_result result = enter(s);
    if (result != OP1_ADAPTER_OK) return result;
    poison(s);
    return OP1_ADAPTER_OK;
}

op1_adapter_result op1_adapter_begin_drain(op1_adapter *s, uint32_t *drain)
{
    uint32_t id;
    op1_adapter_result result = enter(s);
    if (result != OP1_ADAPTER_OK) return result;
    if (s->poisoned != 0u) return OP1_ADAPTER_POISONED;
    if (drain == NULL) return OP1_ADAPTER_INVALID;
    if (s->raw == NULL) return OP1_ADAPTER_NOT_RESERVED;
    if (s->drain_id != 0u) return OP1_ADAPTER_BUSY;
    result = next_id(s, &id);
    if (result != OP1_ADAPTER_OK) return result;
    s->drain_id = id;
    *drain = id;
    return OP1_ADAPTER_OK;
}

op1_adapter_result op1_adapter_release_after_drain(op1_adapter *s, uint32_t drain)
{
    op1_adapter_result result = enter(s);
    if (result != OP1_ADAPTER_OK) return result;
    if (s->poisoned != 0u) return OP1_ADAPTER_POISONED;
    if (s->raw == NULL) return OP1_ADAPTER_NOT_RESERVED;
    if (drain == 0u || drain != s->drain_id) return OP1_ADAPTER_WRONG_DRAIN;
    if (any(s->writers, OP1_ADAPTER_MAX_WRITERS) ||
        any(s->holds, OP1_ADAPTER_MAX_HOLDS)) return OP1_ADAPTER_BUSY;
    s->in_callback = 1u;
    s->release(s->allocator_context, s->raw);
    s->raw = NULL;
    s->capacity = 0u;
    s->drain_id = 0u;
    s->in_callback = 0u;
    /* A callback violation cannot undo a free already committed by that callback. */
    return s->poisoned != 0u ? OP1_ADAPTER_POISONED : OP1_ADAPTER_OK;
}
