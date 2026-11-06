<!-- Working notes for a card that changes this kanban app itself (the "kanban-app" template). The
     template's starting prompt points at this file as {prompts}\kanban-app.md. Where this file and
     a column prompt ({prompts}\planning.md, {prompts}\in-progress.md, {prompts}\ship-it.md,
     {prompts}\deploy-complete.md) disagree, this file wins. -->

# A kanban-app card

<!-- toc -->
**Contents**

- [What this card is](#what-this-card-is)
- [Planning](#planning)
- [In progress](#in-progress)
- [Human test](#human-test)
- [Ship it](#ship-it)
- [Deploy complete](#deploy-complete)
<!-- /toc -->

## What this card is

This card changes the kanban app itself (this repo). The column prompts are written for a web
product with a dev stack and a deploy; follow them for the card tools, the board's rules, the scope
gate, the card-tester, stacks and hand-over, but **not** their dev-stack and deploy parts. Follow
the repo's `AGENTS.md` throughout: it holds the build, the test version and the hard rules.

Not for this card, whatever the column prompt says:

- no worktree, no dev stack, no `logins`, no `devStack`, no browser profiles;
- no deploy-run proofs, no production checks, no after-deploy steps.

You are probably running inside the live app. **Never quit, restart or `-Now` the live app**: it
would kill this very session and every other card's.

## Planning

1. Work in the repo's own checkout on `main`. No worktree, no branch. Record `worktreePath` (the
   checkout) and `branch` = `main` on the card.
2. **One kanban-app card builds at a time**, because they share the checkout. Check
   `kanban_list_cards`: if another card for this repo is In progress (or Human test with
   uncommitted work in the checkout), say so on the card and in your message, and wait in Planning.
3. Read `AGENTS.md`, `HANDOVER.md`, and the code the change touches. Run `git status`: if the
   checkout has someone else's uncommitted work, don't touch it; say so and wait.
4. A GitHub issue is welcome but optional; put it in `issue` if you make one.
5. Write the approach into `summary` (every deliverable listed) and the first build step into
   `nextStep`. Make your own decisions; ask the owner only what is genuinely theirs.

## In progress

1. Build on `main` in the checkout. Don't push.
2. Tests for every data rule and migration; `cargo test --workspace` must pass (see AGENTS.md).
3. Run `/code-review` on your diff and fix what it finds.
4. Write the undo into `UNDO.md`; update `README.md`/`HANDOVER.md` if behaviour changed.
5. Install it in the **test app**: `powershell -ExecutionPolicy Bypass -File scripts/install.ps1 -Channel test`
   (add `-FreshCopy` only if the test board needs to be current). Never the live channel here.
6. Verify it yourself in the test app, driving it with `tools/cdp.mjs` on port 9334 (always
   `--background`). Use dry-run cards on the test board. When the change shows on screen, save a
   shot (`node tools/cdp.mjs shot <png>`) and put it in your message as
   `![what it shows](C:/path/to/shot.png)`, so the owner sees it in the card's chat.
7. Then have the `card-tester` subagent test it ({prompts}\in-progress.md, *Test it with a
   subagent*): brief it with the card number, the deliverables, the commit and the test app on port
   9334, and fix what it finds. The move to Human test is refused until its verdict is a pass.
8. Commit on `main` locally (a clear message). **Don't push** until the owner has tested it.
9. Route: no diff goes to Done; otherwise hand over (below) and move to Human test.

## Human test

Before the move, write into `nextStep`:

- one line: "Installed in the test app, commit `<short sha>`";
- **numbered steps of what to click in the test app** (where to start, what to do, what a pass looks
  like);
- a last line **"Broken if: ..."**.

End your message with the same steps. No logins and no dev stack. If the owner reports a bug, log it
and fix it.

## Ship it

Only after the owner moves the card there, or the app moves it there for them because the card was
set to go on by itself to Deploy complete.

1. `kanban_ship_result` `shipping`.
2. Make sure the tested commit is what's on `main` (`git status` clean for your files, tests pass).
3. `git push origin main`.
4. Stage the live update: `powershell -ExecutionPolicy Bypass -File scripts/install.ps1` (no
   `-Channel`; its default is to stage). **Never `-Now`, never restart or quit the live app
   yourself.** Check that `update\update.json` in the live app's data folder (under `~`) names your
   commit.
5. Log the commit and the staged installer on the card. `kanban_ship_result` `shipped`, with the
   short sha as detail; `failed` with what failed if the push or the build failed.
6. Move to Deploy complete, and tell the owner to **click Restart now** on the "Update ready" bar
   when it suits them (it suspends and resumes every session, this one included). Then numbered
   steps to check it in the live app.

## Deploy complete

Leave the checkout clean for your files (commit real work, delete junk you made, never someone
else's), log what you did, and say where it stands. No after-deploy steps. The owner moves it to
Done.
