# Windows screen-reader pass

`PLAN.md` Milestone 41 asks for a recorded manual pass with the host's own
assistive technology before a backend is called complete. The automated half —
roles, names, states, relationships, patterns, live regions, and focus order read
back through UI Automation — runs in CI (`docs/guarantees.md`). This half needs a
person, and **no pass has been recorded yet**: the Windows backend's
accessibility is verified by the automated suite only, and this document says so
rather than claiming otherwise.

## How to record a pass

Run `examples/reference-app` (and `examples/hello-label` for the wider control
set) with **Narrator** (Win+Ctrl+Enter), and for each item note pass/fail and
what was heard:

1. **Window**: its title is announced on activation.
2. **Tab order**: Tab and Shift+Tab visit every control in reading order;
   nothing unreachable, nothing skipped.
3. **Names and roles**: each control is announced with its label and role
   ("Full name, edit", "Save, button", "Profile, tab item, 1 of 2").
4. **Labelled fields**: a field announces its visible label (`labelled_by`).
5. **State**: disabled and selected states are announced.
6. **Invoke**: Space/Enter activates the focused button; Narrator's own
   invoke action does too.
7. **Scan mode** (Caps Lock+Space): headings and text are readable in
   order.
8. **Live region**: a live-region change (hello-label's async status) is
   announced without moving focus.
9. **Custom-drawn content**: a canvas's virtual elements (hello-label's
   input lab chart) are reachable and announce their names.
10. **High contrast and text size**: with high contrast on and text at
    150 %, every control is readable and nothing is clipped.

## Recorded passes

| Date | Windows build | Narrator version | Tester | Result | Notes |
|---|---|---|---|---|---|
| — | — | — | — | not yet recorded | — |
