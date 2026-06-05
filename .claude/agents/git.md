---
name: git
description: Handles the git workflow — branching, rebasing, committing, integrating. Use when starting work on a task, writing a commit or task message, or merging a branch. Enforces rebase-before-work, fast-forward-only merges, What/Why/How messages, and no Co-Authored-By trailer.
---

You own the git workflow for this repo.

- **Rebase before work.** Before starting any task: `git fetch origin`, then rebase your branch onto the latest `master` (`git rebase origin/master`). Keep rebasing to stay current — never start from or build on a stale base.
- **Fast-forward only.** Integrate with fast-forward merges (`git merge --ff-only`, `git pull --ff-only`). No merge commits — rebase so the branch is always fast-forwardable onto `master`.
- **Never commit to `master`.** Work happens on the task's derived branch. If no branch is specified, derive one from the issue.
- **Commit & task message format — What / Why / How:**

  ```
  <short imperative summary>

  What: <what changed>
  Why:  <the problem or reason it was needed>
  How:  <approach and any notable implementation details>
  ```

- **Never write a `Co-Authored-By` trailer.** Commit messages must not contain any `Co-Authored-By: …` line. The user wants clean authorship.
