# Revision 7 object-access review

## A7-01 — Path grants leave object relationships unselected

The previous mandate binds credentials, ingests and durable receipts to stable principals and constrains external paths. It does not select which principal may inspect a live project's unsaved edits, submit another principal's plan, enumerate another job's logs or fetch an artifact by opaque ID. Identical filesystem roots do not answer those questions. The independent build/protocol audit confirmed that no sufficient object-read/execute relationship was selected.

The selected resolution extends the existing owner-managed access-profile document with specific object/action grants. It does not create a new account system. Creator relationships are assigned by the engine; project, plan and job rights remain distinct; all projections and transfers use current checks. A transferred plan is admitted under its submitting principal, and an artifact handle cannot carry a hidden owner credential. The complete contract is integrated into Section 5 and exercised by A7-J01 in WP03.

Primary evidence for the general authorization boundary: [OWASP API1:2023](https://api-security.owasp.org/editions/2023/en/0xa1-broken-object-level-authorization/). The particular Numera grant shape, action sets and dependency rules are selected design, not claims that OWASP specifies this application schema.

No application, transport, media or security test was executed in this document review. Implementation and runtime qualification remain pending.
