<!-- Ship it column: an example prompt. The column's prompt points at this file as
     {prompts}\ship-it.md. Copy it to your private prompts folder and tailor the merge, deploy and
     proof steps to your repo. -->

# Ship it

<!-- toc -->
**Contents**

- [The card](#the-card)
- [0. Stop the dev stack](#0-stop-the-dev-stack)
- [1. Commit](#1-commit)
- [1a. Bring the branch up to date in the merge lane](#1a-bring-the-branch-up-to-date-in-the-merge-lane)
- [2. Merge the PR, and prove it](#2-merge-the-pr-and-prove-it)
- [3. Watch the deploy](#3-watch-the-deploy)
- [4. Write the docs (commit locally, don't push yet)](#4-write-the-docs-commit-locally-dont-push-yet)
- [5. Record it on the GitHub issues](#5-record-it-on-the-github-issues)
- [6. Prove the deploy](#6-prove-the-deploy)
- [7. Finish](#7-finish)
<!-- /toc -->

Merge this card's PR, prove the deploy, and write the docs while the deploy runs. The owner moved
the card here, or the app moved it here for them because the card was set to go on by itself to
Deploy complete: either is the owner's approval to merge.

Where a step depends on the product (how to stop its dev stack, how it deploys, how to check
what production serves), use the repo's documented way (its README, AGENTS.md or CLAUDE.md). Put
the steps for your own repos in your private prompts folder.

## The card

Your card lives in the kanban app. Use its `kanban_*` tools (in Bash, `kb`). There is no other
board.

- `kanban_get_card` reads it (pr, branch, worktreePath, devStack, logins, links, ship) and its log;
  `kanban_update_card` changes fields; `kanban_add_log` is the card's comment thread (anything this
  file says to "post on the card" goes there); `kanban_notify` tells the owner.
- `kanban_ship_result` reports the ship: `shipping` when you start, `shipped` once both proofs in
  step 6 pass, `failed` with what failed, or `needs-hugh` when only the owner can unblock it. The
  app counts the tries: a `failed` sends the card back to In progress with your detail, up to three
  times, and after the third keeps it here for the owner. So never move the card backwards yourself.
- `kanban_move_card` moves it, with a one-line reason. A move of your own card waits until your
  turn ends.

If your card leads a stack (`kanban_stack_list`), it ships the whole stack in one PR: every child
is on the lead's branch, and the steps below run once, for the lead. Report on the lead only. When
you move the lead to Deploy complete, the app moves its children with it; don't move them yourself.

The owner can drop a card on your card while it waits in Ship it, or before you merge. That is
approval to ship that card in the same merge, and the app's message tells you which branch to merge
in. Give its branch to the merge lane in step 1a (`branches`), so it goes on top of yours and the
checks run on both, and close its PR in favour of yours.
Then, just before step 2's merge, check `kanban_stack_list`: every card in the stack must be on your
branch, so the whole stack ships as one merge, one push and one deploy. Once your ship has started,
the app doesn't add cards to it. A card dropped then waits in Ship it and ships next.

Report `shipping` now, before step 0.

## 0. Stop the dev stack

Testing on the branch ended in Human test, and a stack left up holds memory the next card's
verification needs. Stop it the way In progress started it (the repo's documented way).

A `no-login` card was reviewed rather than signed into, and may have no stack: then there is
nothing to stop.

**One push rule.** After you merge the code PR, nothing else goes to `main` until the deploy run it
started has finished. Prove the one run your merge started, by its id, and never "the latest green
run on main": that may be somebody else's push. Check the run's `conclusion`, not the exit code of
`gh run watch --exit-status`, which can return 0 for a cancelled run. A cancelled run is replayed
with `gh run rerun <id>`.

## 1. Commit

Commit any outstanding code the way the repo documents (including any version bump its deploy
needs). Don't write the release docs yet.

## 1a. Bring the branch up to date in the merge lane

Every ship's merge goes through its repo's merge lane (cards #160, #162): one session the app
owns for each repo. It rebases each ship onto the latest `main` in its own worktree, one at a
time, and keeps what it learnt for the next. It never pushes: you do. Don't rebase or merge the
branch yourself.

If the card has no branch (nothing to merge: its deliverable is data, or isn't code on a branch),
say so on the card, skip 1a and step 2, and prove the deliverable as step 6c says.

1. Commit everything first: `kanban_merge` refuses a worktree with uncommitted work. Then call
   `kanban_merge`, with `branches` (the branch of any card the owner dropped on this one, in
   order), `rules` (the repo's merge rules: how to resolve a version number, a changelog or other
   files that always conflict, in the repo's own words from its CLAUDE.md, AGENTS.md or README)
   and `checks` (its check commands: lint, typecheck, tests, build). Then end your turn. The app
   sends you the lane's verdict and every line of what it changed.
   - If `kanban_merge` refuses because it found no checkout for the repo, report `needs-hugh`
     with its reason, and stop. Fix any other refusal it names (commit your work, set `branch`
     or `worktreePath`) and call it again.
2. On **PASS**, the app's message gives the lane's commit (`head`), the commit your branch was at
   (`base`) and what it rebased onto (`onto`). In your worktree:
   - `git status --porcelain` must be empty and `git rev-parse HEAD` must still be `base`.
     Otherwise your branch moved while the lane worked: report `needs-hugh` and stop.
   - `git reset --hard <head>`.
   - `git range-diff <onto> <base> <head>` must show each of your commits, with only the
     conflict resolutions the lane listed. If one is missing or changed in a way it didn't list,
     report `needs-hugh` and stop.
   - Push the branch: `git push --force-with-lease origin <branch>`.
3. Then do step 2 as written, with the branch already pushed: 2a opens the PR if none is open
   yet. If 2b finds the PR `CONFLICTING` (main moved again), call `kanban_merge` again and do
   this step's 2 again, instead of rebasing yourself. Then 2c and 2d as usual.
4. On **FAIL**, or "Merge unclear" on the card, every branch is where it was (the lane never
   moves one). Report `needs-hugh` with `kanban_ship_result` and its reason, and stop. Don't
   resolve it yourself and don't push.

## 2. Merge the PR, and prove it

a. Push the branch. If no PR is open, open one with `Closes #<n>` in the body, and put it on the
   card (`pr`).
b. Check `gh pr view <n> --json mergeable,mergeStateStatus`. If it is `CONFLICTING` (main moved
   since the lane's merge), go back to 1a: call `kanban_merge` again, and push its commit as 1a
   says. Don't rebase it yourself: the lane resolves the conflicts and reruns the checks.
c. If the deploy keys off a version number, check it even when the merge is clean: another PR may
   have shipped the same version while you were building, and the merge would then deploy nothing.
d. `gh pr merge <n> --merge`, then confirm `state` is `MERGED`: the CLI can print nothing and leave
   the PR open. Record the run id of the deploy the merge started, and log it on the card.
e. If it won't merge, stop: report `needs-hugh` with why. A card must never leave Ship it with its
   PR unmerged, because that is how work silently never ships.

## 3. Watch the deploy

Record the run id, then do steps 4 and 5 while it runs.

**Never wait on a background watcher to tell you the run finished.** A backgrounded `gh run watch`,
Monitor or any background shell is killed when your turn or your session ends, and the notification
you are waiting for never arrives. Poll the run yourself, in the foreground, and treat the run's own
status as the only signal:

```bash
gh run view <run-id> --repo <owner>/<repo> --json status,conclusion \
  --jq '"\(.status)/\(.conclusion // "-")"'
```

Re-check it between steps, and again at the start of every turn, until `status` is `completed`. If
you have to end a turn while it is still running, say so in your message and quote the run id, so
the next turn resumes the poll instead of assuming a notification is coming.

Give it a polling budget of about three times a normal deploy, from the run's own `createdAt`. Only
once the budget is exhausted do you stop: report `needs-hugh` with the run id, the state of every
job, and which ones were still running. Not `failed`: an unfinished deploy is unproven, not failed.

## 4. Write the docs (commit locally, don't push yet)

Cover the whole card, not just the last diff: re-read the card's log and the session history for
bugs found in In progress, root causes, decisions and anything that diverged from the plan.

- The changelog, with one entry. Note any bug fixed where the repo tracks bugs.
- A design note for a real product decision. Fix any doc this card proved wrong.
- AGENTS.md / CLAUDE.md, the README or the area guides, if architecture or conventions changed.

## 5. Record it on the GitHub issues

Comments only, no pushes. On this card's issue, say what was built, what changed from the plan and
what a human must verify. The merge closes the issue, so this is the record. On other affected
issues, say what this resolves and the root causes, and close any that are resolved, with the
reason. New work found along the way becomes a new issue.

## 6. Prove the deploy

Wait until the run is `completed` (by polling it yourself, never by waiting on a background
watcher), then prove it. Do this every time, unasked. A green run is not proof.

First, work out which part of the system this card changed. Many workflows skip jobs by path, so a
job skipping for a part the diff didn't touch is expected; failing a card over it is a false
failure.

a. `gh run view <id> --json jobs`: the job that deploys this card's part must be `success`, not
   skipped.
b. Production must serve it. Check what is live the way the repo documents (the running version or
   image tag, or the changed page or endpoint itself), not just that the pipeline went green. Better
   still, run a check that fails against production before the deploy and passes after.
c. **If the card's deliverable is data rather than code**, (a) and (b) don't prove it. Ask what the
   card asked for, and whether a user can see it on production right now. If you can't check it,
   the card doesn't move: report `needs-hugh` and say why.

If the run was cancelled, `gh run rerun <id>` replays it on the code commit; then prove (a) and (b)
again.

## 7. Finish

**On success** (both proofs pass):
1. Push the docs the way the repo takes docs changes (a docs PR, say) and merge them.
2. Log the deployed version on the card.
3. Add the production address to the card's `links` (`{label: "Production", url}`); its logins
   were cleared in step 0.
4. Report `shipped` with `kanban_ship_result`, with the version as detail.
5. Move the card to Deploy complete with `kanban_move_card`.
6. Tell the owner the card is complete and deployed. End on numbered how-to-test steps against
   production, never a status table.

**On failure** (the build is red, or a post-deploy check rolled production back): leave the docs
unpushed (the deploy isn't real) and report `failed` with exactly what failed (the job, the error,
which check). The app sends the card back to In progress with that detail, or, on the third
failure, keeps it here for the owner: then summarise every attempt and how each failed, and stop.

Never move the card backwards yourself, for any reason.
