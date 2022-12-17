#!/bin/bash

set -eux

# check variables
error=0
if [ -z "${GIT_URL:-}" ]; then
  echo "Error: GIT_URL environment variable not set" > /dev/stderr
  error=1
fi

if [ -z "${GIT_TARGET_BRANCH:-}" ]; then
  echo "Error: GIT_TARGET_BRANCH environment variable not set" > /dev/stderr
  error=1
fi

if [ -z "${GIT_USER_NAME:-}" ]; then
  echo "Error: GIT_USER_NAME environment variable not set" > /dev/stderr
  error=1
fi

if [ -z "${GIT_USER_EMAIL:-}" ]; then
  echo "Error: GIT_USER_EMAIL environment variable not set" > /dev/stderr
  error=1
fi

if [ -z "${HARVESTER_CONFIG:-}" ]; then
  echo "Error: HARVESTER_CONFIG environment variable not set" > /dev/stderr
  error=1
fi

if [ $error -eq 1 ]; then
  echo "aborting ..." > /dev/stderr
  exit 1
fi

echo "${HARVESTER_CONFIG}" > config.json
harvester --config config.json ${HARVESTER_ARGS:-}
git config --global user.name "$GIT_USER_NAME"
git config --global user.email "$GIT_USER_EMAIL"
git remote show tokenpush 2> /dev/null || git remote add tokenpush $GIT_URL
git add result/
git commit -m'rules update'
git push tokenpush $GIT_TARGET_BRANCH
git remote remove tokenpush
