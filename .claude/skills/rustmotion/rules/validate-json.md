# Rule: Always Validate Generated JSON

Every generated JSON scenario MUST be validated with `rustmotion validate` before presenting to the user.

1. Write JSON to a temporary file (e.g. `/tmp/scenario.json`)
2. Run `rustmotion validate /tmp/scenario.json`
3. If validation fails: correct errors and re-validate
4. If validation succeeds: present to the user

**FORBIDDEN:** Presenting JSON that has not been validated by `rustmotion validate`.
