# The benchmark host (plan 13.3)

This page says how to set up a host for `rb bench` as plan 13.3 asks, and where the measured runs go today.

## Where the runs go today

Plan 13.3 names server3 as the benchmark host. server3 cannot be that host now:

- server3 is itself a KVM guest (`systemd-detect-virt` gives `kvm`). It has no `/dev/kvm`, so QEMU can run the BSD guests only under TCG. Times under TCG say nothing about the hardware (13.5).
- Its 8 vCPUs are an "AMD EPYC Processor (with IBPB)" model of the hypervisor, with 1 thread per core. The host below server3 sets SMT and the CPU frequency, not server3. `/sys/devices/system/cpu/smt/control` gives `notsupported`, and there is no `cpufreq` directory.
- Its root file system has about 3 GB free of 387 GB. Two images, their overlays and the bench work need about 30 GB.
- Other agents build on it, so the rule "no other guest and no build during a run" cannot hold.

So the weekly workflow `bench` (`.github/workflows/bench.yml`) runs `rb bench` and `rb speed --bench` on the GitHub runners, which have KVM. A runner is a shared VM, so the noise is higher than on a quiet host. The CV gate of 13.3 handles that: a benchmark with a CV above 1% on G is reported, not gated. The workflow boots R and G in turn on the same runner, so a slow runner slows both sides.

## The setup, for a host with KVM

These steps make a Linux host fit 13.3. They need root, and a bare metal host or a VM with nested KVM.

1. Turn SMT off: `echo off > /sys/devices/system/cpu/smt/control`. Check that `cat /sys/devices/system/cpu/smt/active` gives `0`.
2. Fix the frequency: `cpupower frequency-set -g performance`, and turn off turbo. On Intel: `echo 1 > /sys/devices/system/cpu/intel_pstate/no_turbo`. On AMD: `echo 0 > /sys/devices/system/cpu/cpufreq/boost`. Check with `cpupower frequency-info`.
3. Keep four cores free of other work for the guest. Boot the host with `isolcpus=2-5 nohz_full=2-5 rcu_nocbs=2-5` on the kernel command line, or move the other work off those cores with `systemctl set-property --runtime -- user.slice system.slice init.scope AllowedCPUs=0,1,6-7`.
4. Pin the run to those cores, with a guest of 4 vCPUs and 8 GB. QEMU's threads keep the CPU set of the command.
   - NetBSD: `taskset -c 2-5 rb bench netbsd-trunk --cpus 4 --memory 8G --g-image G.img --r-image R.img --g-bench rb-bench-g --r-bench rb-bench-r`.
   - FreeBSD: `RB_VM_CPUS=4 RB_VM_MEM=8G taskset -c 2-5 scripts/freebsd/boot.sh IMAGE uefi bench OUTDIR` for each boot, R then G, then `rb bench freebsd-15.1 --g-from ... --r-from ...`.
   - `scripts/bench/build.sh` builds `rb-bench-g` and `rb-bench-r`, as the workflow does.
5. Run no other guest and no build on the host during a run. A run is 10 boots of each side per system.
6. Keep the images, the overlays and `work/bench` on a local disk with 30 GB free.

Record the host with the results in the PR or issue comment: `lscpu`, `uname -r`, `cpupower frequency-info` and the command lines.

## When the host changes

The speeds compare R with G on the same host, so a new host does not move the gates. The absolute times in `runs.tsv` do change, so compare them only between runs on the same host.
