# Architectures

rucc has five back ends:
- `X86_64`;
- `Aarch64`;
- `Riscv64`;
- `X86`;
- `Wasm32`.

The first four are the BSD architectures this folder can claim. Everything else the BSDs support (32-bit ARM, PowerPC, MIPS, SPARC64, m68k, VAX, alpha, hppa, sh3) needs a back end rucc does not have, and is outside the claim until it does (02.6).

## 11.1 The matrix

| Row | FreeBSD | NetBSD | From | Machine for tests |
|---|---|---|---|---|
| amd64 | `amd64`, tier 1 | `amd64` | B3 (NetBSD), B5 (FreeBSD) | KVM on gpc |
| amd64 `lib32` | the 32-bit compatibility libraries, built by `XCC -m32` | `compat/amd64/i386`, `MKCOMPAT` | B6 (FreeBSD), B4 (NetBSD) | the amd64 images |
| aarch64 | `arm64`, tier 1 | `evbarm-aarch64` (`MACHINE=evbarm`, `MACHINE_ARCH=aarch64`) | B7 | QEMU `virt`, under KVM on arm64 hardware if there is any, otherwise TCG (open question 7) |
| riscv64 | `riscv`, tier 2 | `riscv-riscv64`, new in 11.0 | B7 | QEMU `virt`, TCG |
| i386 | `stable/14` only; 15.0 retired the i386 kernel | `i386` | B12 | KVM, 32-bit guests |

**Not claimed:** FreeBSD `armv7`, `powerpc64` and `powerpc64le`; every NetBSD port other than the four above. The reference GCC itself is also broken on FreeBSD 32-bit `arm` and `powerpc` (01), so those would have no G anyway.

**`lib32` comes early.**
- FreeBSD's amd64 world builds `lib32` with the world compiler and `-m32` by default (`MK_LIB32`).
- NetBSD's amd64 build does the same for `compat` when `MKCOMPAT=yes`, which is the default on amd64.

So the i686 back end is graded from the first amd64 world (B4 and B6), long before the i386 rows. `rucc-kernel`'s i686 work (03.8) is what makes that possible.

## 11.2 aarch64

**The kernel flag lines.**

| Flag | FreeBSD (`sys/conf/kern.mk:144-157`) | NetBSD (`sys/arch/aarch64/conf/Makefile.aarch64`) | rucc |
|---|---|---|---|
| no FP or SIMD in the kernel | `-mgeneral-regs-only` | `-march=armv8-a+nofp+nosimd` (`:36`) | accepted (probed) |
| reserved register | `-ffixed-x18`, which holds the per-CPU pointer | none | accepted. `rb vec-audit` extends to "no use of `x18`" |
| branch protection | `-mbranch-protection=standard`, and `-Wl,-zbti-report=error` where the linker supports it | `pac-ret+bti`, `pac-ret` or `bti` by option (`:48-54`) | accepted; emits `paciasp` and `autiasp` and a `.note.gnu.property` with BTI and PAC (probed) |
| atomics | `-mno-outline-atomics` | default | accepted |
| `INLINE_LIMIT` | 8000, which becomes `-finline-limit=8000` (refused today, 03.4) | none | |

**Userland.** FreeBSD builds every program and library with `-mbranch-protection=standard` (`share/mk/bsd.prog.mk:60`, `bsd.lib.mk:136`).

**Three checks specific to aarch64:**
1. **The GNU property note.** The linker ANDs the BTI, PAC and GCS bits across every input. One rucc object without BTI turns BTI off for a whole program or kernel, silently. With `-zbti-report=error` that is a build failure, which is better. clang 21 with `standard` also sets GCS, while rucc does not (probed). Whether GCC 15.2's `standard` sets GCS decides whether that is a difference. B7 reads it from the reference and `rb sections-diff` compares the notes of every object.
2. **`char` is unsigned, `long double` is IEEE quad.** Quad arithmetic goes to `__addtf3` and its family in `libgcc` (NetBSD, built by rucc, 03.14) or `libcompiler_rt` (FreeBSD). A wrong soft-float call name or argument passing convention shows up only in `libm`'s tests.
3. **Memory ordering.** The BSD kernels use their own `atomic_*` with `dmb` and acquire or release instructions, written as inline `asm`. rucc's own atomics are graded by `libthr`, `libpthread` and the kernel locking tests running on more than one vCPU. That is a reason TCG with `-smp 4` is used, even though it is slower.

## 11.3 riscv64

| Flag | FreeBSD (`kern.mk:158-181`) | NetBSD (`sys/arch/riscv/conf/Makefile.riscv`) | rucc |
|---|---|---|---|
| ISA | `-march=rv64imafdch_zifencei_svinval` | the port's default | accepted (probed) |
| kernel ABI | `-mabi=lp64`, soft-float in the kernel, while F and D stay in `-march` for context switch code | same idea | accepted |
| code model | `-mcmodel=medany` for GCC (`:175`) | `-mcmodel=medany` (`:53`) | accepted |
| relaxation | `-mno-relax` when the linker lacks RISC-V relaxation support | default | accepted |
| userland | `-march=rv64imafdc -mabi=lp64d` (`share/mk/bsd.cpu.mk:347`) | `lp64d` | accepted |

**Specific risks:**
- **Relaxation.** With relaxation on, the linker rewrites `auipc` and `jalr` pairs and needs `R_RISCV_RELAX` on exactly the instructions GCC marks. rucc's assembler output must carry the same relaxation relocations. Otherwise a link succeeds and a jump lands in the middle of an instruction.
- **The `h` and `svinval` extensions.** These are in the kernel's `-march` for the hypervisor and TLB code. rucc must accept their instructions in inline `asm` (`hfence.vvma`, `sinval.vma`). The census (07.5) lists them.
- **`rv32` compatibility.** NetBSD sets `MKCOMPAT=yes` on `riscv64` (`share/mk/bsd.own.mk:1157-1163`) and builds a 32-bit `rv32` libc, start files and `ld.elf_so` (`compat/riscv64/rv32/bsd.rv32.mk`). rucc has no riscv32 back end. The NetBSD riscv64 row therefore sets `MKCOMPAT=no` in both R and G (02.1). On aarch64 the same file turns compat on only for clang builds (`HAVE_GCC` unset), so a GCC build of NetBSD aarch64 has no 32-bit compat and needs no knob.
- **No hardware.** The tests run under TCG only. NetBSD's riscv port is new in 11.0, and its ATF run on QEMU is not published, so the baseline is ours (12.5).

## 11.4 i386 (B12)

**NetBSD** `i386` is a full port with GCC as its compiler and a complete ATF run. It uses:
- rucc's `X86` back end;
- `-m32`-style code everywhere, including the kernel;
- `-mregparm` in places.

It is the only 32-bit system kernel in the claim.

**FreeBSD** `stable/14` still has the i386 kernel and world. 15.0 retired them, and `lib32` on amd64 is all that remains on 15. The row lives until `stable/14` leaves support.

i386 comes late because the i686 back end is already graded through `lib32` from B4 and B6. A full i386 system adds:
- the 32-bit kernel, with its PAE and non-PAE page tables, `-mno-sse` and the `gs`-based per-CPU access;
- the 32-bit start files;
- a long tail of 64-bit integer helper calls (`__udivdi3` and family, from `libgcc` or `libcompiler_rt`).

## 11.5 Per-architecture cost

| Architecture | rucc work | Harness work | Test time per pin |
|---|---|---|---|
| amd64 | most of 03 | most of 14 | about an hour per full test run under KVM; FreeBSD CI uses 2 vCPU and 8 GB |
| aarch64 | the property note; checking `x18`; the census of inline `asm` | QEMU images per system; firmware (`QEMU_EFI.fd`) | three to ten times amd64 under TCG |
| riscv64 | relaxation; ISA extension parsing; the census | images; OpenSBI and U-Boot or EDK2 for each system | ten times amd64 or more under TCG |
| i386 | 32-bit kernel flags; helper calls | images | about amd64 |

Under TCG the nightly cannot run the full ATF suite on every pin for aarch64 and riscv64:
- **Nightly:** the boot and smoke suites on aarch64 and riscv64.
- **Weekly:** the full suite on one pin per system.
- **Sharded:** the full suite is split across all builders by test program.

Open question 7 is whether arm64 hardware exists in the fleet. With KVM on arm64, aarch64 becomes an every-night row like amd64.
