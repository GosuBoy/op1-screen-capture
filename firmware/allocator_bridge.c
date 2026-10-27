#include "allocator_bridge.h"

static void *allocate_payload(void *context, uint32_t bytes)
{
    const op1_allocator_bridge *bridge = context;
    return bridge->allocate(bytes);
}

static void release_payload(void *context, void *raw)
{
    const op1_allocator_bridge *bridge = context;
    bridge->release(raw);
}

op1_adapter_result op1_adapter_bind_allocator_bridge(
    op1_adapter *adapter, op1_allocator_bridge *context)
{
    if (context == NULL || context->allocate == NULL || context->release == NULL) {
        return op1_adapter_bind(adapter, NULL, NULL, NULL);
    }
    return op1_adapter_bind(adapter, allocate_payload, release_payload, context);
}
