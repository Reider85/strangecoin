#!/bin/bash
# Helper script to push without TTY prompts
# [GITHUB-P33] CI5 regression guard: activate repo hooks before push.
git config core.hooksPath .githooks
# Auth: never hardcode tokens here (this file is committed to a public repo).
# Run `gh auth login` once (scopes: repo, workflow) — gh then provides git
# credentials non-interactively.
git config --global credential.helper "store --file=/tmp/.git_credentials"
echo "Using credential store"
git push origin github-p09-pages --force