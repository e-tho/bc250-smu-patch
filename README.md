# bc250-smu-patch

Applies firmware patches to the AMD BC-250 SMU. The patches correct telemetry for the two factory-disabled cores once [unlocked](https://github.com/rw-r-r-0644/bc250-core-unlock).

> [!IMPORTANT]
> The shipped patches target BIOS P3.00 only.

## Usage

Run as root.

> [!WARNING]
> On a failed run, the SMU can stay unresponsive until a cold power cycle.

```
bc250-smu-patch [--check] [--hex PATH]
```

- `--check` unlocks the SMU if not already open, then reads every site and reports whether it matches.
- `--hex PATH` reads a hex file other than the embedded one.

Exit status is 0 when every site matched or applied cleanly, 1 otherwise.

## How it works

`bc250-smu-patch` reaches the SMU through the root complex's PCI config space. It opens the debug gate if closed, reads each patch site, writes the ones that differ through the gated path, and verifies each write by reading it back.

## Motivation

`bc250-smu-patch` reimplements the unlock sequence and the patch application of [bc250-smu-unlock](https://github.com/rw-r-r-0644/bc250-smu-unlock). Since the reference implementation is written in Python, running it in the initrd requires including the language runtime, which adds over 100 MB uncompressed, while this binary occupies a few hundred kB. A smaller initrd unpacks faster, uses less memory during early boot, and leaves more room on the boot partition.

## Credits

[bc250-smu-unlock](https://github.com/rw-r-r-0644/bc250-smu-unlock) by [rw-r-r-0644](https://github.com/rw-r-r-0644).

## License

MIT
