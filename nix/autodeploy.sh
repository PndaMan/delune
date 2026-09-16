# Deploys a NixOS flake when it or one of its inputs changes, checks the
# machine is still healthy, and puts the last good system back if it isn't.
#
# Settings arrive as environment variables from the systemd unit (see
# autodeploy.nix). Runs as root; one run at a time.

set -euo pipefail

state=${STATE_DIR:-/var/lib/autodeploy}
mkdir -p "$state"
exec 9>"$state/lock"
flock -n 9 || { echo "another deploy is running"; exit 0; }

cd "$REPO"
export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.directory GIT_CONFIG_VALUE_0="$REPO"

log() { echo "$*"; }
last_problem=""

notify() {
  # $1 title, $2 body, $3 priority
  [[ -n "${NTFY_URL_FILE:-}" && -r "$NTFY_URL_FILE" ]] || return 0
  curl -fsS --max-time 10 -H "Title: $1" -H "Priority: ${3:-default}" -H "Tags: $HOST" \
    -d "$2" "$(<"$NTFY_URL_FILE")" >/dev/null || true
}

failed_units() {
  systemctl list-units --state=failed --plain --no-legend | awk '{print $1}' | sort
}

# Healthy means: every named unit is active, no unit failed that wasn't failed
# before the switch, and every check command passes. Checks get a few tries,
# since services take a moment to come up.
healthy() {
  local before=$1 tries=${HEALTH_TRIES:-6} i unit check new
  for ((i = 1; i <= tries; i++)); do
    sleep "${SETTLE_SECONDS:-20}"
    local bad=""
    for unit in ${UNITS:-}; do
      systemctl is-active --quiet "$unit" || bad+="unit $unit is $(systemctl is-active "$unit" || true); "
    done
    new=$(comm -13 "$before" <(failed_units) | tr '\n' ' ')
    [[ -n "$new" ]] && bad+="newly failed: $new; "
    while IFS= read -r check; do
      [[ -z "$check" ]] && continue
      timeout 30 bash -c "$check" </dev/null >/dev/null 2>&1 || bad+="check failed: $check; "
    done <"${CHECKS_FILE:-/dev/null}"
    if [[ -z "$bad" ]]; then
      return 0
    fi
    log "not healthy yet ($i/$tries): $bad"
    last_problem=$bad
  done
  return 1
}

activate() {
  nix-env -p /nix/var/nix/profiles/system --set "$1"
  timeout "${SWITCH_TIMEOUT:-900}" "$1/bin/switch-to-configuration" switch
}

rollback() {
  local good=$1 reason=$2
  log "rolling back to $good: $reason"
  if activate "$good"; then
    notify "$HOST: deploy rolled back" "$reason" high
  else
    notify "$HOST: rollback FAILED" "$reason. The system may need a hand; last good system is $good" urgent
  fi
}

# A deploy that was interrupted (a reboot or a crash mid-switch) is finished
# here: kept if healthy, rolled back if not.
if [[ -f "$state/pending" ]]; then
  good=$(<"$state/pending")
  before="$state/failed-before"
  [[ -f "$before" ]] || : >"$before"
  if healthy "$before"; then
    log "interrupted deploy turned out healthy"
    readlink -f "${CURRENT_SYSTEM:-/run/current-system}" >"$state/good"
  else
    rollback "$good" "an interrupted deploy wasn't healthy: $last_problem"
    [[ -f "$state/pending-lock" ]] && cp "$state/pending-lock" flake.lock
  fi
  rm -f "$state/pending" "$state/pending-lock"
fi

known_bad() { grep -qxF "$1" "$state/bad" 2>/dev/null; }

# 1. The repo itself: follow its upstream when the tree is clean.
if [[ "${PULL:-1}" == 1 ]] && git rev-parse --abbrev-ref '@{u}' >/dev/null 2>&1; then
  if git diff --quiet && git diff --cached --quiet; then
    git fetch --quiet
    upstream=$(git rev-parse '@{u}')
    if [[ "$(git rev-parse HEAD)" != "$upstream" ]] && ! known_bad "repo=$upstream"; then
      git merge --ff-only --quiet '@{u}' || log "can't fast-forward to upstream; leaving the repo as it is"
    fi
  else
    log "the repo has uncommitted changes; not pulling"
  fi
fi

# Whether GitHub's checks passed for a commit: prints pass, fail or wait.
ci_state() {
  local slug=$1 rev=$2 auth=() body
  [[ -n "${GITHUB_TOKEN_FILE:-}" && -r "$GITHUB_TOKEN_FILE" ]] && auth=(-H "Authorization: Bearer $(<"$GITHUB_TOKEN_FILE")")
  body=$(curl -fsS --max-time 20 "${auth[@]}" -H "Accept: application/vnd.github+json" \
    "https://api.github.com/repos/$slug/commits/$rev/check-runs?per_page=100") || { echo wait; return; }
  jq -r '
    .check_runs as $runs
    | if ($runs | length) == 0 then "wait"
      elif any($runs[]; .status != "completed") then "wait"
      elif all($runs[]; .conclusion == "success" or .conclusion == "skipped" or .conclusion == "neutral") then "pass"
      else "fail" end' <<<"$body"
}

# 2. Inputs that should follow their upstream.
cp flake.lock "$state/lock-before"
changes=()
for input in $INPUTS; do
  node=$(jq -r --arg i "$input" '.nodes[.root].inputs[$i] | if type == "array" then last else . end' flake.lock)
  locked=$(jq -r --arg n "$node" '.nodes[$n].locked.rev // empty' flake.lock)
  owner=$(jq -r --arg n "$node" '.nodes[$n].original.owner // empty' flake.lock)
  name=$(jq -r --arg n "$node" '.nodes[$n].original.repo // empty' flake.lock)
  ref=$(jq -r --arg n "$node" '.nodes[$n].original.ref // "HEAD"' flake.lock)
  if [[ -z "$owner" || -z "$name" ]]; then
    log "$input isn't a github: input; skipping"
    continue
  fi
  remote=$(git ls-remote "https://github.com/$owner/$name" "$ref" | awk 'NR==1{print $1}')
  if [[ -z "$remote" || "$remote" == "$locked" ]] || known_bad "$input=$remote"; then
    continue
  fi
  if [[ "${REQUIRE_CI:-1}" == 1 ]]; then
    case $(ci_state "$owner/$name" "$remote") in
      wait) log "$input ${remote:0:7}: waiting for its checks"; continue ;;
      fail) log "$input ${remote:0:7}: checks failed; skipping it"
            echo "$input=$remote" >>"$state/bad"
            notify "$HOST: skipped $input ${remote:0:7}" "Its CI checks didn't pass." default
            continue ;;
    esac
  fi
  nix flake update "$input" --refresh
  now=$(jq -r --arg n "$node" '.nodes[$n].locked.rev // empty' flake.lock)
  if [[ "$now" != "$remote" ]]; then
    log "$input locked to ${now:0:7}, expected ${remote:0:7}; will retry"
    cp "$state/lock-before" flake.lock
    continue
  fi
  changes+=("$input=$remote")
done

# 3. Build what the repo describes now, and stop if it's what's running.
# Nothing to do when the tree is exactly as it was on the last attempt.
tree_state() { { git rev-parse HEAD; git diff HEAD; cat flake.lock; } | sha256sum | cut -d' ' -f1; }
rev=$(git rev-parse HEAD)
fingerprint=$(tree_state)
if [[ -f "$state/tried" && "$(<"$state/tried")" == "$fingerprint" ]]; then
  exit 0
fi
echo "$fingerprint" >"$state/tried"
if ! new=$(nix build --no-link --print-out-paths ".#nixosConfigurations.$HOST.config.system.build.toplevel" 2>"$state/build.log"); then
  tail -n 30 "$state/build.log"
  cp "$state/lock-before" flake.lock
  for c in "${changes[@]}" "repo=$rev"; do echo "$c" >>"$state/bad"; done
  notify "$HOST: deploy didn't build" "$(tail -n 8 "$state/build.log")" high
  tree_state >"$state/tried"
  exit 1
fi
# Keep the updated lock, so the tree stays clean and can keep following upstream.
commit_lock() {
  [[ ${#changes[@]} -gt 0 && "${COMMIT_LOCK:-1}" == 1 ]] || return 0
  git -c user.name=autodeploy -c user.email="autodeploy@$HOST" \
    commit --quiet --only flake.lock -m "deploy: update ${changes[*]}" || return 0
  tree_state >"$state/tried"
  if [[ "${PUSH:-0}" == 1 ]]; then git push --quiet || log "couldn't push the lock"; fi
}

current=$(readlink -f "${CURRENT_SYSTEM:-/run/current-system}")
if [[ "$new" == "$current" ]]; then
  [[ ${#changes[@]} -gt 0 ]] && log "inputs moved but the system is the same"
  commit_lock
  exit 0
fi

# 4. Switch, check, and keep or roll back.
summary="${changes[*]:-repo ${rev:0:7}}"
log "deploying $new ($summary)"
before="$state/failed-before"
failed_units >"$before"
echo "$current" >"$state/pending"
cp "$state/lock-before" "$state/pending-lock"

last_problem=""
if ! activate "$new"; then
  last_problem="switching failed"
fi
if [[ -z "$last_problem" ]] && healthy "$before"; then
  echo "$new" >"$state/good"
  rm -f "$state/pending" "$state/pending-lock"
  commit_lock
  notify "$HOST: deployed" "$summary" low
  log "deployed"
  exit 0
fi

rollback "$current" "${summary}: ${last_problem}"
cp "$state/lock-before" flake.lock
for c in "${changes[@]}"; do echo "$c" >>"$state/bad"; done
[[ ${#changes[@]} -eq 0 ]] && echo "repo=$rev" >>"$state/bad"
rm -f "$state/pending" "$state/pending-lock"
# The restored tree is the one that's running; don't build it again.
tree_state >"$state/tried"
exit 1
