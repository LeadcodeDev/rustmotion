# Rule: Timing Constraints

When both `start_at` and `end_at` are specified, `start_at` must be strictly less than `end_at`. Both values are in seconds relative to scene start.

**BAD:**
```json
{ "start_at": 2.0, "end_at": 1.0 }
```

**GOOD:**
```json
{ "start_at": 0.5, "end_at": 2.5 }
```

Also: scene `duration` must be > 0, and at least one scene is required.
