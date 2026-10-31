// SPDX-License-Identifier: Apache-2.0
//
// The per-test timeout of the tests whose time is file I/O, not computation:
// those that read the whole research archive through the read-only /research
// bind mount (tests/research.ts), and the ones that walk src/ file by file
// (tests/theme-script.test.ts, and the literal success/green variant scan of
// src/components/ in tests/provenance.test.ts, which took 9 to 11 s under
// load in review SNG1-01/SNG2-1). Vitest's default is 5 s. On an idle machine
// these tests take about 2 s, but bind-mount reads slow down with host load:
// under a load average of 47 the manifest pin test took 12.6 s and the src/
// walk 21.8 s, 23 s and once 93 s (review CHAIN-5+GATES-05), so the 5 s
// default failed `pnpm check` with no code defect. 120 s covers the worst run
// measured and still stops a test that hangs. It is given to those tests
// alone, as their third argument: every other test keeps the default, so a
// slow computation elsewhere still fails fast.

export const IO_BOUND_TEST_TIMEOUT_MS = 120_000;
