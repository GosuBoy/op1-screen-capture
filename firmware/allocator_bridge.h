#ifndef OP1_CAPTURE_ALLOCATOR_BRIDGE_H
#define OP1_CAPTURE_ALLOCATOR_BRIDGE_H

#include "adapter.h"

/* Ordinary C functions with one argument, not context-first callbacks. */
typedef void *(*op1_single_argument_alloc_fn)(uint32_t bytes);
typedef void (*op1_single_argument_free_fn)(void *raw);

/*
 * Unbound ABI description only: no firmware addresses or runtime discovery.
 * Initialize both fields before binding; keep this object, both functions and
 * their allocator arena valid and immutable for the entire bound adapter life.
 * In particular, releasing the current reservation does NOT unbind the adapter
 * or permit replacing/destroying this context. There is no rebind/reset API.
 * The context must be disjoint from the adapter, output objects and allocated
 * storage. Do not modify it from an allocator callback or another execution
 * context. External serialization and adapter.h's trusted-callback contract apply.
 * Correct function types do not prove a stock address, ABI, execution context,
 * heap lifetime, executable placement or target hardware compatibility.
 */
typedef struct {
    op1_single_argument_alloc_fn allocate;
    op1_single_argument_free_fn release;
} op1_allocator_bridge;

/*
 * Bind explicit payload-forwarding callbacks once. NULL context/functions fail
 * without calling an allocator; a live adapter's existing binding is retained.
 * As with adapter_bind, detected callback reentry poisons the adapter, including
 * attempts to bind an invalid context. No pointer is converted to a different
 * function type and no capture functionality is enabled.
 * All non-NULL input pointers must denote valid objects of the declared types.
 */
op1_adapter_result op1_adapter_bind_allocator_bridge(
    op1_adapter *adapter, op1_allocator_bridge *context);

#endif
