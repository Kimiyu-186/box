# Temporary quarantine

**Status: active. One Linux case remains disabled.**

| Case | Platform | Reason | Owner |
| --- | --- | --- | --- |
| CN-W-06 | Linux | The Cargo fixture declares unsupported tool `exec` lists and needs supported grants and verified toolchain paths. | box-maintainers |

CN-W-05 now runs the Git workflow with supported readable and writable grants.
It checks commit-hook execution, agent execution refusal, unconfigured Git refusal, read-only files, protected configuration, and an ungranted program.
The Cargo exception remains under the [leaf toolchain decision](../docs/design/decisions.md#a-leaf-runs-its-whole-toolchain-and-loads-what-it-builds).

## How the exception works

`src/quarantine.rs` holds the exact compiled list and the required skip message.
The pinned unit test permits only `[("CN-W-06", "linux")]`.
The runner records an authorized skip without running that case.
Platform declarations still apply before the quarantine check.

The reducer accepts that skip only when its message matches the compiled entry.
A quarantined case that runs, an unlisted skip, a missing result, or another failure keeps the verdict red.
The report records the remaining entry and `coverage_gate = "reduced: 1 quarantined cell(s) on this platform"` on Linux.
macOS has no quarantine entries.
A green Linux result therefore remains reduced coverage, not complete qualification.

## Restore the Cargo case

Convert the Cargo fixture to supported helper grants and verify its toolchain paths.
Remove its entry from `TEMPORARY_QUARANTINE` and update the pinned test and reducer tests.
Remove the quarantine comment above its case.
Run the full Linux deterministic suite and require the restored case to pass.
Resolve this document only when the compiled quarantine list is empty.

The runner tests prove that restored Git runs and quarantined Cargo skips.
The reducer tests retain refusal of incorrect skip messages, unlisted skips, missing or duplicate results, and command failures.
