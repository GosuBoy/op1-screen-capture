/* ARTIFICIAL LINKER TEST ONLY. 0x1000 is not an OP-1 placement proposal.
 * No output produced with this script is authorized for loading or flashing.
 * Deliberately minimal, read-only module subset for check-placement's ELF gate.
 * Unknown allocated/writable sections, unaccounted LOAD padding, missing symbol
 * tables or invalid linker symbols will be rejected by that gate.
 */
ENTRY(_op1_adapter_bind)
SECTIONS
{
  . = 0x1000;
  .text : { *(.text .text.*) }
  .rodata : { *(.rodata .rodata.*) }
  /DISCARD/ : { *(.comment) }
}
