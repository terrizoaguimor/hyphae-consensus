// SPDX-License-Identifier: Apache-2.0
//! Admission controls for the public live mode: the USD budget ledger,
//! Turnstile verification, curated presets and the geographic policy.
//! Nothing here calls a model: these modules decide whether a visitor's
//! request may spend the operator's key at all. Filled in by lane S-b2;
//! the public API (lane S-b1) wires them in afterwards.
