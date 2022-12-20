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

# prepare target directory
REPO_DIR=${HARVESTER_REPO_DIR:-/var/cache/harvester/output}
CACHE_DIR=${HARVESTER_CACHE_DIR:-/var/cache/harvester/cache}
mkdir -p "$REPO_DIR"
cd "$REPO_DIR"

# pull or clone git repo
if [ -d .git ]; then
  git checkout $GIT_TARGET_BRANCH || git checkout -b $GIT_TARGET_BRANCH
  git remote show tokenupstream 2> /dev/null || git remote add tokenupstream $GIT_URL
  git pull tokenupstream $GIT_TARGET_BRANCH || true
else
  git clone $GIT_URL .
  git remote show tokenupstream 2> /dev/null || git remote add tokenupstream $GIT_URL
  git checkout $GIT_TARGET_BRANCH || git checkout -b $GIT_TARGET_BRANCH
fi
git config --global user.name "$GIT_USER_NAME"
git config --global user.email "$GIT_USER_EMAIL"

# make sure not to write into the wrong directories
echo "${HARVESTER_CONFIG}" \
  | jq -r --arg out_format ${HARVESTER_OUT_FORMAT:-Lua} '.output_format |= $out_format' \
  | jq -r --arg cache_dir "${CACHE_DIR}" '.cache_dir |= $cache_dir' \
  | jq -r '.output_dir |= "./"' \
  > /tmp/config.json
# run harvester
harvester --config /tmp/config.json ${HARVESTER_ARGS:-}

# commit and push results
git add .
git commit -m'rules update'
git push tokenupstream $GIT_TARGET_BRANCH
git remote remove tokenupstream
