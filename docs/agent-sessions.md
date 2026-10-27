# Save agent conversation references

These optional integrations save explicit conversation identities for Claude
Code, Codex and Pi. They do not report status or resume agents. The shared
`scripts/agent_session.py` uses Python 3's standard library and the `hc` on PATH.
Copy it to a stable location on each machine where agents run; the desktop app
does not install these scripts and does not need to be open.

## Claude Code and Codex

Add this entry to the agent's existing `hooks.SessionStart` array, keeping its
other hooks. For Codex, use `~/.codex/hooks.json`; for Claude Code, use
`~/.claude/settings.json` and change the last argument to `claude`.

```json
{
  "hooks": [
    {
      "type": "command",
      "command": "python3 /absolute/path/agent_session.py codex",
      "timeout": 10
    }
  ]
}
```

On Windows, use the installed Python executable and quote paths in the command:

```json
"command": "py -3 \"C:\\path\\agent_session.py\" codex"
```

The adapters accept root `SessionStart` metadata. Explicit subagent events are
ignored; new, resumed,
cleared and forked root conversations add their references. Repeated reports
of the exact same reference are harmless. Existing sessions need another SessionStart
event before they are captured. Follow the agent's hook approval/reload process
after editing its configuration.

The tested Codex CLI `0.164.0-fork.2` emits this hook at the first turn, before
the first model request. Opening its prompt without starting a turn does not
capture an ID. Actual upstream Codex and Claude execution remain unverified.

## Pi

Copy `scripts/pi_session.ts` beside `agent_session.py` and load it explicitly:

```sh
pi --extension /absolute/path/pi_session.ts
```

Pi's native extension loader runs the TypeScript directly. Python 3 must be on
PATH as `python` on Windows or `python3` on Linux/macOS, alongside `hc`.
For persistent loading, add the extension through Pi's normal extension
configuration; loading this file does not configure Claude or Codex hooks.

The extension uses Pi 1.1.0's native `session_start` event for startup, reload,
new session, resume and fork. It captures `getSessionId()`, `getSessionFile()`
and `getCwd()` from `ctx.sessionManager`, without reading conversation contents.
The event has no root/subagent discriminator; each explicit session reference
reported within the terminal is retained. `--no-session` has no session file
and is skipped. A new persistent session can report its allocated path before
Pi creates the file on the first conversation message, so a saved reference
alone does not prove that resumable history exists yet.

Actual Pi 1.1.0 CLI startup, new session, resume and fork were verified with a
synthetic session fixture and no model prompts. Linux/macOS execution and Pi
reload execution remain unverified.

## Persistence and limits

The integrations run only inside a live core terminal. `HC_TERMINAL_SOCKET`
selects that terminal's own core, even if `HC_SOCKET` selects another session.

The core stores a document named `herdr-agent-t7` for terminal `t7`, containing
`references`, a list of distinct `{agent, session_id, cwd}` records; Pi records
also contain `session_file`, the explicit file reference for `pi --session`.
Overlapping
hooks merge their references with version checks, so a late old hook cannot
erase a newer conversation. The list does not identify an active/latest session.
A successful hook means the core acknowledged
the write to disk; a failure returns a nonzero exit code and a short error.
The document belongs to that core and survives full stop, reboot and holder loss.
It does not contain prompts, transcripts, command arguments or environment data.
The core's existing 1 MiB document limit still applies. At that limit a new
reference fails visibly; the helper never silently evicts an older reference.

This saves the mapping for recovery; it does not resume the agent. Kept terminals
still return as shells. Use the saved ID with the agent's own resume command in
the original pane. Disabling keep, ending or deleting a terminal does not launch
anything or erase the reference document. Automatic resume and reference cleanup
remain separate client policy; a saved reference is not evidence that a session
is still eligible to run.

Producer contracts: [Codex](https://learn.chatgpt.com/docs/hooks),
[Claude Code](https://code.claude.com/docs/en/hooks), and
[Pi extension API](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/src/core/extensions/types.ts).

Run the owning tests with `python3 -B -m unittest discover -s scripts -p test_agent_session.py`.
Set `HC_AGENT_TEST_CORE` to an `hc` executable to also exercise real PTYs, full
stop, simultaneous core/holder termination and a failed persistence write. These
probes use their own temporary home and core session. No model requests are made.
