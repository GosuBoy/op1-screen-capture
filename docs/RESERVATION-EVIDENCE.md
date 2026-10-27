# Physical reservation: evidence and remaining gates

The offline checks now establish the bounded relationship between the supplied
adapter object and its linked code. They do not establish an owned executable
region on the device. This static audit selects **no concrete physical placement**,
produces no deployable layout and leaves capture permanently disabled.

## What is established, conditionally

### Startup ordering and allocator exclusion

The initial heap descriptors are loaded before their normal runtime registration.
A cold descriptor change can restrict that allocator's endpoint; changing the
initial table after registration cannot revoke existing allocations or rewrite its
live state. Startup includes an extensive global-constructor phase before kernel
initialization and thread execution. "Before the main thread runs" is therefore
not a sufficient reservation boundary by itself.

The inspected arena-registration routine checks duplicate IDs, but does not
compare arena intervals for overlap. Excluding a tail from one arena does not
prevent a differently identified overlapping arena, a direct writer or DMA from
using it. A syntactic direct-call search narrows the known registration paths;
it is not complete closure of indirect calls or aliases.

The inspected loader declarations narrow explicit write ranges. They do not
establish indirect boot scratch, DMA lifetimes or the selected physical boot
path. Earlier writes are not necessarily conflicts if final code is loaded
subsequently and the previous owner has ceased using the region. The proof must
cover ordering and lifetime, not merely whether the region was ever touched.

### Intended instruction mapping exists

The successful normal startup path installs instruction and data descriptors that
cover the low-SDRAM range containing the heap-tail proposal. The relevant
instruction mapping is cacheable; the data mapping is cacheable and write-back.
This narrows the question from "is the range mapped at all?" to "are the intended
mapping and code-visibility conditions actually established at the chosen phase?"

Those static tables are not a readback of the physical device. They do not prove
all setup/error paths succeed, later table state is unchanged or instruction
fetches see newly written bytes. A descriptor's replacement-policy lock is not
memory ownership or protection from later explicit changes.

A cold image completed before a proven cache-initialization boundary is different
from copying code through the runtime data path. The former needs final-load,
inherited-state and initialization ordering; the latter needs an exact data-cache
writeback, instruction-cache invalidation, synchronization and execution-exclusion
contract. This audit implements neither strategy. See the architecture's
[instruction/data cache guidance](https://www.analog.com/media/en/dsp-documentation/processor-manuals/Blackfin_pgr_rev2.2.pdf);
that guidance does not replace target-specific silicon and lifecycle evidence.

### Capacity is not a free-space calculation

Configured boot stack requests and identified allocation sites supply lower-bound
requirements, not a maximum for stock operation. An identified large runtime allocation
path also requests storage from the same heap; its reachability and lifetime
relative to capture remain unbounded. Request sizes alone do not establish
simultaneous lifetimes, peak live use, allocation success, retained headroom or
fragmentation.

Consequently, subtracting known stack requests and the current adapter size from
nominal RAM would not justify a reservation. The complete hook/trampoline and
initialization path must also be sized; adapter state, immutable bridge context
and frame staging need separately owned allocations with failure handling.

## Smallest evidence needed before selecting a layout

1. **Load phase and owner lifetimes:** identify the actual boot/load path and every
   relevant owner that can write after final code is loaded, including callbacks,
   scratch buffers, DMA and restart paths. Establish when each owner stops using
   the proposed region.
2. **Arena and writer closure:** rule out later overlapping registrations and
   direct/indirect static or DMA aliases. A direct-call search or unused-looking
   loader range is insufficient.
3. **Complete integration image:** provide the actual startup hook, trampoline,
   initialization and failure paths, with final code size and loading time.
4. **Code visibility:** prove the selected mapping and cache-coherence sequence
   for that loading phase, including inherited state and relevant silicon limits.
5. **Stock capacity envelope:** establish peak retained heap/stack requirements,
   supported-mode allocation lifetimes, stack-safety bounds, allocator overhead
   and fragmentation/free-extent allowances, rather than interpreting a lower
   bound as available capacity.

The address-neutral validator can check a proposed plan against declared budgets
and evidence references; it cannot make these missing premises true. A matching
manifest and correct relocation results are necessary checks, not physical
placement approval. No firmware, device or Mac changes were made by this audit.
