#!/bin/sh
# Builds and inspects synthetic links only. Never execute/load/flash target code.
set -eu
cd "$(dirname "$0")/.."
: "${BFIN_CC:=bfin-elf-gcc}"
: "${BFIN_LD:=bfin-elf-ld}"
export BFIN_CC
cargo run --offline --bin check_adapter
out=target/relocation-synthetic
mkdir -p "$out"
for base in 0x1000 0x8000 0xfffc 0x12341000 0x7fff1000 0xfffff000; do
    sed "s/\. = 0x1000;/\. = $base;/" examples/placement-synthetic.ld > "$out/link-$base.ld"
    # Link the exact REL object checked below, not separately rebuilt constituents.
    "$BFIN_LD" -n -T "$out/link-$base.ld" -o "$out/linked-$base.elf" target/adapter-check/adapter-bf524.o
    cargo run --offline --bin check_relocations -- target/adapter-check/adapter-bf524.o "$out/linked-$base.elf"
    REL_INPUT=target/adapter-check/adapter-bf524.o LINKED_INPUT="$out/linked-$base.elf" \
        cargo test --offline --test relocation_adversarial actual_artifact_semantics_and_corruptions -- --ignored --exact
    sha256sum target/adapter-check/adapter-bf524.o "$out/linked-$base.elf"
done
echo 'Six artificial links checked; corruption rejection tested. No physical placement or hardware proof.'
