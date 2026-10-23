# Herdr app

The Herdr 2 desktop app (`herdr-app`; code name was Bliss): a native GPUI
client of herdr-core. Lives in `~/Projects/herdr-app` (`~/try/bliss` links
here for sessions started before the move). Read `.brain/README.md` first
(state, code map, how to work here, where everything else is).

## Not released: no backward compatibility

Only Can and his agents use it. Until the first release, rename and
reshape freely: no aliases for old names, no migration code, no reading
old labels, files or formats "so existing setups keep working". When a
change breaks Can's own `~/.config/herdr-app` or his running terminals'
labels, say so and fix his copy by hand with his OK. Old-name aliases
already in the code can be deleted when you touch them. Remove this
section at release.

## Before you grow the app's core: ask Can

The app's core (the model, `spaces.json`, commands, what every mod gets)
stays small; everything else is mods. Before adding anything to it, answer
these three, then discuss with Can and wait for his go:

1. Does the app itself act on it (it changes what the app does, not only
   how something looks)?
2. Can only the app see it (e.g. a change that happens while no mod is
   watching)?
3. Can mods not do it correctly, even with care?

If all three are no, it belongs in mods (their shared storage, their own
code), even at the price of some repetition. Same for herdr-core: what the
app asks of it goes through Can too.

## The guide users' agents read

`defaults/AGENTS.md` is written into every user's `~/.config/herdr-app` on
every start: it's how their agents learn what the app can do. A change to
what users or agents can do (a command, a setting, a layout or mod
feature, a file) updates it in the same commit. `cargo test` checks that it
names every command and settings.json key; the rest is on you. Keep it a
map with rules and short examples: the `.d.ts` files are generated and are
the full API.

## Branches, Herdr Dev and Can's Herdr

Big work happens on a branch in a worktree
(`~/Projects/herdr-app-worktrees/<name>`, its `target` a link into
`/var/tmp`), so the main checkout's build, which is Can's Herdr, stays
stable. Can tries a branch in **Herdr Dev**: the desktop app "Herdr Dev",
`~/.local/bin/herdr-app-dev`, a separate app with its own home
(`~/.config/herdr-app-dev`), settings and core session (`herdr-app-dev`).
Point its script at the branch's build. Never point it, or anything, at
Can's `~/.config/herdr-app`.

Commit only when Can says go. "Move my Herdr" means: bring the branch into
the main checkout and build it there, so his own Herdr runs it.

## The dogfooding loop

Can lives in the app while you work, and your own session usually runs
inside it. His terminals live in herdr-core, so quitting and reopening the
app never kills them.

1. Change code, then `cargo build` (the sandbox runs it). When it's ready
   for Can, `cargo build --release` in the main checkout:
   `~/.local/bin/herdr-app` (and the GNOME app "Herdr", both set up by
   `scripts/install-linux`) runs `target/release/herdr-app`. Never point
   that link anywhere else (a worktree, a branch's build): Can's Herdr
   runs the main checkout, only Herdr Dev runs branches.
2. Test on the sandbox, never on Can's app: `scripts/sandbox up`, then
   `do <command> [json]`, `status`, `shot` (screenshot), `move|click|drag`
   (pointer input, window px), `type`, `key <keysym> [Control_L ...]`,
   `hc ...`, `restart`, `down`. It has its own core, home and a headless
   display (Weston + Xwayland) under `/var/tmp/herdr-app-sandbox`: nothing
   pops up on Can's screen. Never fake input on his display (GNOME asks him
   for remote-control permission and it moves his real pointer).
3. Tell Can it's ready. He quits and reopens the app; you keep running.

Never run a bare `target/*/herdr-app` (no arguments) without
`HERDR_APP_HOME` pointing at a sandbox: it opens a second window on Can's
real setup. Subcommands and `--help` only talk.

On macOS (`ssh mac`, repo still at `~/try/bliss` there), run `scripts/install-macos` after
syncing changes. It builds and installs a native executable inside
`~/Applications/Herdr.app` (with the Herdr icon), signs it locally and
registers it with macOS. `~/.local/bin/herdr-app` points to that bundled
executable. Never replace it with a shell wrapper: macOS needs the
executable's bundle identity. The installer does not quit or reopen the app.

Helpers: start sub-agents with herdr-core, not a harness's subagent tool
(we dogfood `hc`): `hc run --name <name> --cwd <dir> gpt-implementer
"<task>"` (Pi on gpt-6.1-sol; templates in `~/.config/herdr-core/templates`,
`gpt-reviewer` for reviews). They show up as your sub-agents in Can's
app; `hc wait`, `hc read`, `hc send` to work with them.

Rules:
- Never stop, signal or restart Can's app or his core (`hc server ...`
  without `HC_SESSION`). `herdr-app status` / `ls` against his window are
  fine (read only).
- `defaults/` is what ships in the box: mods, `layouts/*.json`, `keys.json`
  (Linux) and `keys.macos.json` (Mac). The app builds them in. Mods update
  in `~/.config/herdr-app` on start if Can didn't edit them (and go when no
  longer shipped). Layouts update by merge and keep his edits
  (`layout_sync.rs`; files older than that stay as they are). `keys.json`
  is installed only when missing, so it's his copy: when you change a
  default key, ask whether to change his file too.
- Remove sandboxes (`scripts/sandbox down`) when done.
