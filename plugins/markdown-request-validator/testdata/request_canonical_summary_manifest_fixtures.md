Canonical summary manifest fixtures use JSON input and expected JSON pairs:

| Case | Input | Expected output |
| --- | --- | --- |
| Valid request | `request_canonical_summary_manifest_success_input.json` | `expected_request_canonical_summary_manifest_success_payload.json` |
| Equivalent formatting (reordered frontmatter/sections, whitespace and CRLF) | `request_canonical_summary_manifest_equivalent_input.json` | `expected_request_canonical_summary_manifest_equivalent_payload.json` |
| Malformed frontmatter | `request_canonical_summary_manifest_malformed_input.json` | `expected_request_canonical_summary_manifest_malformed_payload.json` |
| Tracker-style scope identifier (`dot3925`, derived from `DOT-3925`) | `request_canonical_summary_manifest_tracker_scope_input.json` | `expected_request_canonical_summary_manifest_tracker_scope_payload.json` |

The equivalent input is intentionally a formatting-only variant of the successful request; the two expected payloads are identical byte-for-byte after compact JSON serialization.
