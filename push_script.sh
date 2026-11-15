#!/bin/bash
# Helper script to push without TTY prompts
git config --global credential.helper "store --file=/tmp/.git_credentials"
echo "Using credential store"
echo "Username: Reider85"
echo "Token: [вставить токен или использовать gh auth]"
git push origin github-p09-pages --force