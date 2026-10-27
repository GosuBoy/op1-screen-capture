#ifndef OP1_CAPTURE_DISABLED_ADAPTER_H
#define OP1_CAPTURE_DISABLED_ADAPTER_H

#include <stddef.h>
#include <stdint.h>

/* Software bounds only, not physical heap capacity or a hardware owner count. */
#define OP1_ADAPTER_MAX_STAGING UINT32_C(524288)
#define OP1_ADAPTER_MAX_WRITERS 8u
#define OP1_ADAPTER_MAX_HOLDS 4u
#define OP1_ADAPTER_INITIALIZER {0}

typedef void *(*op1_adapter_alloc_fn)(void *context, uint32_t bytes);
typedef void (*op1_adapter_free_fn)(void *context, void *raw);

typedef enum {
    OP1_ADAPTER_OK = 0,
    OP1_ADAPTER_INVALID,
    OP1_ADAPTER_UNBOUND,
    OP1_ADAPTER_ALREADY_BOUND,
    OP1_ADAPTER_BUSY,
    OP1_ADAPTER_SIZE,
    OP1_ADAPTER_NO_MEMORY,
    OP1_ADAPTER_WITHDRAWN,
    OP1_ADAPTER_POISONED,
    OP1_ADAPTER_UNKNOWN_LEASE,
    OP1_ADAPTER_CAPACITY,
    OP1_ADAPTER_ID_EXHAUSTED,
    OP1_ADAPTER_WRONG_DRAIN,
    OP1_ADAPTER_NOT_RESERVED,
    OP1_ADAPTER_REENTRANT
} op1_adapter_result;

/*
 * OFFLINE PROTOTYPE: no built-in enable, pixel-copy, USB or MMIO operation.
 * Allocator callbacks are trusted allocator-only adapters; arbitrary supplied
 * callbacks could perform other effects and are outside this contract.
 * Zero-initialize ONCE before first use. Never copy, memset, reset, or directly
 * edit a live instance. Fields are public for freestanding storage, not an API.
 * Handles belong to this instance and must not cross instances or actual boots.
 * All calls require external serialization and a proved ordinary-thread context.
 * This is not an interrupt-safe lock or real writer interception implementation.
 * Output pointers must name valid caller-owned uint32_t objects disjoint from
 * the adapter storage. Arbitrary/aliased pointer safety is not provided.
 * Callbacks must not mutate or copy adapter fields directly. They must return
 * normally, preserve their C ABI, and not reenter this API.
 * Detectable mutator reentry poisons the instance while callbacks own the state.
 * A nonnull allocation returned after that violation is retained, not forgotten.
 * A free callback that violates the contract may already have freed memory;
 * detection cannot undo that free. No concurrency,
 * nonlocal-exit recovery, heap lifetime or allocation alignment is established.
 */
typedef struct {
    uint32_t bound;
    uint32_t in_callback;
    uint32_t withdrawn;
    uint32_t poisoned;
    uint32_t last_id;
    uint32_t drain_id;
    void *raw;
    uint32_t capacity;
    op1_adapter_alloc_fn allocate;
    op1_adapter_free_fn release;
    void *allocator_context;
    uint32_t writers[OP1_ADAPTER_MAX_WRITERS];
    uint32_t holds[OP1_ADAPTER_MAX_HOLDS];
} op1_adapter;

/* Bind immutable adapters, not raw stock function addresses of a different ABI. */
op1_adapter_result op1_adapter_bind(op1_adapter *, op1_adapter_alloc_fn,
                                    op1_adapter_free_fn, void *context);
op1_adapter_result op1_adapter_reserve(op1_adapter *, uint32_t bytes);
/* Always zero, including for NULL; this milestone cannot admit capture. */
int op1_adapter_capture_allowed(const op1_adapter *);

/* Instrumentation bookkeeping only. Rejection is NOT proof a real writer stopped. */
op1_adapter_result op1_adapter_begin_writer(op1_adapter *, uint32_t *lease);
op1_adapter_result op1_adapter_end_writer(op1_adapter *, uint32_t lease);
/* Synthetic consumer lease; this API exposes no staging pointer or pixels. */
op1_adapter_result op1_adapter_begin_hold(op1_adapter *, uint32_t *lease);
/* End means all accesses by that consumer have actually stopped. */
op1_adapter_result op1_adapter_end_hold(op1_adapter *, uint32_t lease);

/* In-process withdrawal only: retain raw allocation and all outstanding leases. */
op1_adapter_result op1_adapter_invalidate(op1_adapter *);
/* Unknown ownership permanently poisons this instance and prevents reclamation. */
op1_adapter_result op1_adapter_fault(op1_adapter *);

/* Drain request blocks new holds, but still records writers. No waiting occurs. */
op1_adapter_result op1_adapter_begin_drain(op1_adapter *, uint32_t *drain);
/*
 * Caller confirms all accesses, including untracked platform accesses, stopped
 * and the original allocator arena/context remain valid. Requires matching drain
 * and no tracked writers/holds. Calls exact original free(raw) once. A request,
 * timeout, mode change or hardware reset is NOT this confirmation. Poisoned
 * instances cannot start reclamation; no recovery/reset is provided here.
 * If reentry is detected inside the free callback, POISONED may be returned
 * after that callback already freed storage. It is never permission for a
 * manual retry: the returned callback is recorded exactly once and raw cleared.
 */
op1_adapter_result op1_adapter_release_after_drain(op1_adapter *, uint32_t drain);

#endif
