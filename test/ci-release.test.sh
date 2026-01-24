#!/usr/bin/env bash
set -euo pipefail

if [ "$(git branch --show-current)" != "develop" ]; then
  echo "This script must be run from the 'develop' branch."
  exit 1
fi

git add .
git commit --allow-empty -m "ci:test"
git push origin develop

gh pr create --base master --head develop --title "ci:test" --body "Trigger CI" --fill
gh pr merge --merge

git switch master
git pull origin master

PWD_ROOT="${PWD%%/tuliprox*}/tuliprox"

cd $PWD_ROOT
./bin/release.sh minor