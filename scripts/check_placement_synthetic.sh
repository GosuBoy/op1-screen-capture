#!/bin/sh
# Offline synthetic integration test only. Never load/flash the outputs.
set -eu
cd "$(dirname "$0")/.."
: "${BFIN_CC:=bfin-elf-gcc}"
: "${BFIN_LD:=bfin-elf-ld}"
: "${BFIN_READELF:=bfin-elf-readelf}"
export BFIN_CC
cargo run --offline --bin check_adapter
out=target/placement-synthetic
mkdir -p "$out"
"$BFIN_LD" -T examples/placement-synthetic.ld -o "$out/linked.elf" target/adapter-check/adapter-core-bf524.o target/adapter-check/allocator-bridge-bf524.o
printf 'SYNTHETIC CANONICAL INPUT; NOT FIRMWARE\n' > "$out/canonical.txt"
canonical=$(sha256sum "$out/canonical.txt" | cut -d ' ' -f 1)
module=$(sha256sum target/adapter-check/adapter-bf524.o | cut -d ' ' -f 1)
linked=$(sha256sum "$out/linked.elf" | cut -d ' ' -f 1)
entry=$("$BFIN_READELF" -h "$out/linked.elf" | awk '/Entry point address:/ {print $4}')
cat > "$out/manifest" <<MANIFEST
version=1
canonical_sha256=$canonical
module_sha256=$module
linked_sha256=$linked
heap.start=0x100
heap.bytes=0x1f00
heap.alignment=8
usable_start=0x140
cut=0xff0
retained_stock_budget=1024
code.start=0x1000
code.bytes=0x400
code.alignment=4
guard_before.start=0xff0
guard_before.bytes=16
guard_before.alignment=4
guard_after.start=0x1400
guard_after.bytes=16
guard_after.alignment=4
final_image_bytes=0x400
entry=$entry
state_bytes=92
bridge_bytes=8
staging_bytes=256
runtime_alignment=4
allocation_overhead_budget=128
reservation_stage=before_first_heap_registration
image_inventory=final_module_hooks_trampolines_init
runtime_contract=zeroed_state_immutable_bridge_lifetime_disjoint_serialized_fail_closed
evidence.boot_owners=synthetic-only-not-proof
evidence.alias_dma_owners=synthetic-only-not-proof
evidence.cache_execution=synthetic-only-not-proof
evidence.stock_capacity=synthetic-only-not-proof
evidence.startup_hook=synthetic-only-not-proof
evidence.link_provenance=synthetic-only-not-proof
MANIFEST
cargo run --offline --bin check_placement -- "$out/manifest" "$out/canonical.txt" target/adapter-check/adapter-bf524.o "$out/linked.elf"
# Pin the actual relocatable bytes as the purported final artifact, so rejection
# tests ELF type/relocations rather than merely a mismatched digest.
sed "s/^linked_sha256=.*/linked_sha256=$module/" "$out/manifest" > "$out/rel-manifest"
if cargo run --offline --bin check_placement -- "$out/rel-manifest" "$out/canonical.txt" target/adapter-check/adapter-bf524.o target/adapter-check/adapter-bf524.o > "$out/rejected-rel.log" 2>&1; then
    echo 'ERROR: relocatable artifact incorrectly accepted' >&2
    exit 1
fi
grep 'expected ordinary Blackfin ET_EXEC' "$out/rejected-rel.log"
sed '/^evidence\./d' "$out/manifest" > "$out/incomplete-manifest"
if cargo run --offline --bin check_placement -- "$out/incomplete-manifest" "$out/canonical.txt" target/adapter-check/adapter-bf524.o "$out/linked.elf" > "$out/rejected-incomplete.log" 2>&1; then
    echo 'ERROR: incomplete manifest incorrectly accepted' >&2
    exit 1
fi
grep 'missing review references' "$out/rejected-incomplete.log"
echo 'Synthetic link envelope accepted; relocatable and incomplete plans rejected. No hardware proof.'
