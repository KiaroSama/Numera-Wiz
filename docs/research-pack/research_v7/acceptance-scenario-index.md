# Cross-domain acceptance index

These 63 planned scenarios retain the previous 34 journeys and add 29 focused contracts, including the complete architecture and explicit MuxCls rewrite. They supplement Annexes A/D and per-domain/effect/format fixtures; none is a completed runtime test. Full assertions and joins are in the CSV and the named mandate sections.

| Scenario | Purpose | Requirements | Work packages | CI gates | Main location |
|---|---|---|---|---|---|
| PJ01 | Portable first useful task | R06;R11;R15;R19;R21 | WP02;WP03;WP05 | D6-G03 | Section 11 / Product acceptance |
| PJ02 | Shared editor intake | R11;R13;R14;R19 | WP04 | D6-G03 | Section 11 / Product acceptance |
| PJ03 | Source to timeline | R11;R13;R19 | WP06 | D6-G03 | Section 11 / Product acceptance |
| PJ04 | Edit interval algebra | R11;R18;R19 | WP06 | D6-G01;D6-G03 | Section 11 / Product acceptance |
| PJ05 | Animation through edits | R11;R12;R19 | WP06 | D6-G03 | Section 11 / Product acceptance |
| PJ06 | Proxy then original | R04;R11;R20 | WP04 | D6-G03 | Section 11 / Product acceptance |
| PJ07 | Relink without substitution | R11;R18;R19 | WP04 | D6-G03 | Section 11 / Product acceptance |
| PJ08 | Mixed audio and automation | R11;R19;R20 | WP08 | D6-G03 | Section 11 / Product acceptance |
| PJ09 | Captions and graphics | R10;R11;R19 | WP08 | D6-G03 | Section 11 / Product acceptance |
| PJ10 | Nested and multicam edit | R11;R19 | WP08 | D6-G03 | Section 11 / Product acceptance |
| PJ11 | Concurrent human and agent | R13;R14;R18;R19 | WP03;WP06 | D6-G01;D6-G03 | Section 11 / Product acceptance |
| PJ12 | Reviewed export and retry | R11;R18;R19;R21 | WP03;WP07;WP12 | D6-G03 | Section 11 / Product acceptance |
| PJ13 | Recovery and portable collection | R11;R15;R18 | WP03;WP04;WP12 | D6-G03 | Section 11 / Product acceptance |
| PJ14 | Same working GUI and keyboard | R09;R13;R14;R19 | WP06 | D6-G03 | Section 11 / Product acceptance |
| M6-J01 | Two clips, an overlay, a keyed crop/transform and a nested title | R10;R11;R19;R20 | WP05;WP07;WP08 | D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J02 | Labeled VFR frames at `0,40,100,160,220,300` ms, known extent `330` ms | R10;R11;R20 | WP05;WP07 | D6-G01;D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J03 | Known contiguous 48 kHz audio plus video with a nonzero offset | R11;R18;R20 | WP05;WP07 | D6-G01;D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J04 | Join three segments with video lengths fixed by the request: first has no French audio, second has French and English audio, third has English only | R11;R19;R20 | WP07 | D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J05 | One track feeds both main output and a pre-fader send | R10;R11;R19 | WP08 | D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J06 | Numbered audio impulses through dry and latency-bearing insert routes into one bus | R10;R11;R18;R20 | WP08 | D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J07 | A long sequence whose full source set exceeds the simultaneous decoder budget | R04;R11;R20 | WP05;WP08 | D6-G03;D6-G04 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J08 | Reverse/boomerang across at least three chunks, with a seeded procedural effect and a visible repeated-endpoint policy. Seek and export from the same frozen snapshot. | R04;R11;R18 | WP05;WP07 | D6-G03;D6-G04 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J09 | Incomplete metadata duration, late-discovered stream, deliberately early seeking and a bounded full-scan/materialization cap. | R04;R11;R18 | WP07 | D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J10 | A subtitle cue crosses a removed interval | R10;R11;R19 | WP05;WP07;WP08 | D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J11 | Valid structure with an intentionally damaged interior media region, a metadata-only remux, and a declared compatible bitstream reframing case. | R11;R18;R20 | WP07 | D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J12 | Mixed folder job: encode, stream-copy remux, multiple extracts, one incompatible subtitle, and two files with different final-mix loudness. Exercise separate deliberate-cancel and owner-crash/restart branches after a completed child. | R11;R18;R19;R21 | WP07 | D6-G03;D6-G04 | Section 11 / Compiler and FFmWiz acceptance |
| D6-J01 | Portable launch/repair journey | R04;R05;R15;R18 | WP02 | D6-G03 | Section 8 |
| D6-J02 | Owner/transport lifecycle journey | R08;R14;R18;R19 | WP03 | D6-G03 | Section 4 |
| D6-J03 | Upgrade/rollback journey | R15;R18 | WP12 | D6-G03 | Section 4 |
| D6-J04 | Human/agent setup journey | R06;R07;R08;R19 | WP03;WP11 | D6-G03 | Section 5 |
| D6-J05 | Profile closure journey | R03;R05;R10;R11;R16 | WP02 | D6-G02;D6-G03 | Section 8 |
| D6-J06 | CI completeness journey | R16;R18;R19 | WP12 | D6-G01;D6-G05;D6-G07 | Section 8 |
| D6-J07 | Owner evidence/promotion journey | R05;R16;R17;R22 | WP12 | D6-G05;D6-G06;D6-G07 | Section 8 |
| D6-J08 | Resource evidence journey | R04;R18;R20;R22 | WP10 | D6-G03;D6-G04;D6-G06 | Section 8 |
| P7-J01 | Bounded coherent editor projections | R04;R13;R19 | WP03;WP06 | D6-G03 | 5 / Bounded project projections |
| P7-J02 | Complete headless shortcut registration | R09;R19 | WP03;WP06 | D6-G01;D6-G03 | 7 / Complete shortcut contract |
| P7-J03 | Future project data compatibility | R11;R12;R18 | WP03;WP06 | D6-G03 | 5 / Future project data |
| P7-J04 | Deterministic sequence settings | R11;R19;R20 | WP05;WP06 | D6-G03 | 5 / Sequence setting behavior |
| P7-J05 | Identifiable headless media observations | R08;R19;R20 | WP03;WP05;WP11 | D6-G03 | 5 / Agent media observations |
| MF7-J01 | Temporal processed-prefix and clip-clock taps | R10;R11;R19;R20 | WP05;WP09 | D6-G03 | 6 / Temporal graph prefix |
| MF7-J02 | Nested temporal schedules and resource bounds | R04;R10;R11;R20 | WP05;WP09 | D6-G03;D6-G04 | 6 / Temporal graph prefix |
| MF7-J03 | Variable input epochs and continuous state | R10;R11;R18;R20 | WP05;WP08 | D6-G03 | 6 / Input format changes |
| MF7-J04 | Reconfiguration evidence under log pressure | R18;R21;R22 | WP05;WP10 | D6-G03;D6-G06 | 6 / Input format changes |
| MF7-J05 | Native and browser presentation color | R13;R14;R19;R20 | WP05;WP06 | D6-G03 | 6 / Preview presentation target |
| B7-J01 | Packaged browser CSP/MIME boot | R13;R14;R18;R19 | WP03;WP05;WP06 | D6-G03 | 7 / Packaged browser boot |
| B7-J02 | Pre-probe input and reference admission | R10;R11;R18 | WP02;WP04 | D6-G02;D6-G03 | 6 / Input admission before probe |
| B7-J03 | Bounded authorized observation/artifact bytes | R04;R08;R18;R19 | WP03;WP11 | D6-G03 | 5 / Artifact transfer |
| A7-J01 | Cross-principal object boundaries | R08;R18;R19 | WP03 | D6-G03 | 5 / Object access |
| AR7-J01 | Architecture dependencies and headless boot | R04;R05;R12;R19 | WP02;WP03 | D6-G01;D6-G02;D6-G03 | 4 / Complete implementation architecture |
| AR7-J02 | Shared semantic pipeline for utilities and editing | R10;R11;R19;R21 | WP03;WP05;WP07 | D6-G03 | 4 / Complete implementation architecture |
| AR7-J03 | Cross-process ownership and retired-version replay | R04;R12;R18;R19 | WP03;WP10;WP12 | D6-G03;D6-G05 | 4 / Complete implementation architecture |
| MX7-J01 | Complete selection and per-file resolution | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J02 | Partial order and metadata precedence | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J03 | All preservation categories and strict-copy incompatibility | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J04 | True no-op copy and unchanged media evidence | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J05 | Output trees, native paths and companion semantics | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J06 | No new artifact cannot be success | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J07 | Independent verification catches semantic mismatch | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J08 | Collision, cancellation and recovery boundaries | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J09 | Process faults and finite operation limits | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J10 | Bounded resources, progress and truthful totals | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J11 | Native/browser/CLI/MCP parity | R04;R10;R11;R18;R19;R20;R21 | WP03;WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J12 | Portable CPU-only package and rewrite completion | R04;R10;R11;R18;R19;R20;R21 | WP01;WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
