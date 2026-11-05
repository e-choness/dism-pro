#!/usr/bin/env bash
# Toolchain images pinned by digest in .github/toolchain.lock (ADR-0003, decision #29).
#
#   toolchain-images.sh pull t1 [t2]   pull the pinned images and tag them the way compose.yaml
#                                      expects (dism-pro-build:<target>-local)
#   toolchain-images.sh lock T1 T2     print a lock for this docker/build.Dockerfile and the given
#                                      ghcr.io/e-choness/dism-pro-build@sha256:… refs
#   toolchain-images.sh check-private  fail if anyone can pull the package without signing in
#   toolchain-images.sh check-publish  before a push: check-private, and the package must already
#                                      exist and be reachable with $GHCR_USER / $GHCR_TOKEN
#
# The package is private. Images published since the xwin splat (MSVC CRT + Windows SDK) moved out
# of them hold no Microsoft files, but older versions do, and that splat isn't redistributable.
# Log in to ghcr.io before `pull` (CI: GITHUB_TOKEN with packages: read; a person:
# a token with read:packages). Anyone without access builds the same images from source, which is
# what `docker compose` does on first use.
#
# This repository's workflows never create the package. A package a workflow creates with its
# GITHUB_TOKEN takes the visibility of the workflow's repository, which is public here, and a
# public package can't be made private again. The owner creates the package once, from a PRIVATE
# repository's workflow, so no personal token is made or pasted anywhere:
#   1. Create a private repository (e.g. e-choness/ghcr-seed) with a README.
#   2. Add .github/workflows/seed.yml there:
#        name: seed
#        on: workflow_dispatch
#        permissions:
#          packages: write
#        jobs:
#          seed:
#            runs-on: ubuntu-latest
#            steps:
#              - env:
#                  GHCR_USER: ${{ github.actor }}
#                  GHCR_TOKEN: ${{ secrets.GITHUB_TOKEN }}
#                run: |
#                  echo "$GHCR_TOKEN" | docker login ghcr.io -u "$GHCR_USER" --password-stdin
#                  docker pull hello-world
#                  docker tag hello-world ghcr.io/e-choness/dism-pro-build:seed
#                  docker push ghcr.io/e-choness/dism-pro-build:seed
#   3. Run it (Actions > seed > Run workflow). The new package is private, like that repository.
#   4. In the package's settings: check it says Private, then under "Manage Actions access" add
#      e-choness/dism-pro with the Write role. Keep the seed repository private (archive it).
#   5. Run this repository's toolchain workflow. The seed version can be deleted afterwards.
#
# Exit codes: 0 done; 1 usage or a malformed lock; 3 nothing is pinned for this Dockerfile (build
# from source instead); 4 a pull failed; 5 the package is public; 6 the package doesn't exist yet,
# or the given credentials can't reach it.
set -euo pipefail

readonly PACKAGE=e-choness/dism-pro-build
readonly LOCK=.github/toolchain.lock
readonly DOCKERFILE=docker/build.Dockerfile
readonly REF_RE='^ghcr\.io/e-choness/dism-pro-build@sha256:[0-9a-f]{64}$'

cd "$(git rev-parse --show-toplevel)"

# One line on stderr (so command substitutions never capture it); in GitHub Actions it becomes
# an annotation of the given level.
say() {
  local level=$1
  shift
  if [[ ${GITHUB_ACTIONS:-} == true ]]; then
    echo "::${level} title=toolchain images::$*" >&2
  else
    echo "$level: $*" >&2
  fi
}

# What the images are built from. The Dockerfile copies nothing from its context, so its blob id
# (line endings normalized by .gitattributes) identifies the inputs. A COPY, ADD or bind mount
# would add inputs this doesn't see.
inputs() {
  if grep -qiE '^[[:space:]]*(COPY|ADD)[[:space:]]|--mount=[^[:space:]]*type=bind' "$DOCKERFILE"; then
    say error "$DOCKERFILE now reads files from its context; hash them in $0 and widen toolchain.yml's paths"
    exit 1
  fi
  git hash-object -- "$DOCKERFILE"
}

# The value of KEY in the lock (CRs dropped, for Windows checkouts); exactly one line must set it.
value() {
  awk -F= -v key="$1" '{ sub(/\r$/, "") } $1 == key { print substr($0, length(key) + 2); n++ } END { exit n != 1 }' "$LOCK" ||
    { say error "$LOCK must set $1 exactly once"; exit 1; }
}

pull() {
  local target ref pinned have err start=$SECONDS
  [[ $# -gt 0 ]] || { echo "usage: $0 pull t1 [t2]" >&2; exit 1; }
  for target in "$@"; do
    [[ $target == t1 || $target == t2 ]] || { echo "unknown target: $target" >&2; exit 1; }
  done
  if [[ ! -f $LOCK ]]; then
    say notice "nothing is pinned yet (no $LOCK)"
    return 3
  fi
  pinned=$(value dockerfile)
  have=$(inputs)
  if [[ $pinned != "$have" ]]; then
    say notice "$DOCKERFILE differs from the one the pinned images were built from (blob $have, pinned $pinned)"
    return 3
  fi
  for target in "$@"; do
    ref=$(value "$target")
    [[ $ref =~ $REF_RE ]] || { say error "$LOCK: $target must be a ghcr.io/$PACKAGE@sha256:… ref, not $ref"; exit 1; }
    if ! err=$(docker pull --quiet "$ref" 2>&1 >/dev/null); then
      say warning "couldn't pull $ref (${err//$'\n'/ })"
      return 4
    fi
    docker tag "$ref" "dism-pro-build:$target-local"
  done
  say notice "pulled $* by digest in $((SECONDS - start)) s"
}

lock() {
  local blob
  [[ $# -eq 2 && $1 =~ $REF_RE && $2 =~ $REF_RE ]] ||
    { echo "usage: $0 lock ghcr.io/$PACKAGE@sha256:<t1> ghcr.io/$PACKAGE@sha256:<t2>" >&2; exit 1; }
  blob=$(inputs)
  cat <<EOF
# Toolchain images in GHCR, a private package (ADR-0003, decision #29). The toolchain workflow
# prints this file after it publishes. CI pulls these images while docker/build.Dockerfile is
# still the blob below, and builds them from source otherwise.
dockerfile=$blob
t1=$1
t2=$2
EOF
}

check_private() {
  local token code
  # An anonymous pull token, the same thing `docker pull` asks for without credentials.
  token=$(curl -sS --max-time 30 "https://ghcr.io/token?scope=repository:$PACKAGE:pull" |
    sed -n 's/.*"token":"\([^"]*\)".*/\1/p') || true
  code=$(curl -sS --max-time 30 -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer ${token:-none}" "https://ghcr.io/v2/$PACKAGE/tags/list") || true
  case $code in
    200)
      say error "ghcr.io/$PACKAGE can be pulled without signing in. Its older images hold Microsoft files that aren't redistributable. A package can't be made private again once public: delete it, create it again privately (steps at the top of $0), then run the toolchain workflow"
      return 5
      ;;
    401 | 403 | 404)
      echo "ghcr.io/$PACKAGE isn't readable anonymously (HTTP $code)"
      ;;
    *)
      say error "couldn't tell whether ghcr.io/$PACKAGE is public (HTTP ${code:-none})"
      return 1
      ;;
  esac
}

check_publish() {
  local token code
  [[ -n ${GHCR_USER:-} && -n ${GHCR_TOKEN:-} ]] ||
    { echo "usage: GHCR_USER=… GHCR_TOKEN=… $0 check-publish" >&2; exit 1; }
  check_private || return
  # A pull token for these credentials, the way `docker login` gets one. The credentials go
  # through a curl config on stdin, not the command line.
  token=$(printf 'user = "%s:%s"\n' "$GHCR_USER" "$GHCR_TOKEN" |
    curl -sS --max-time 30 -K - "https://ghcr.io/token?scope=repository:$PACKAGE:pull" |
    sed -n 's/.*"token":"\([^"]*\)".*/\1/p') || true
  code=$(curl -sS --max-time 30 -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer ${token:-none}" "https://ghcr.io/v2/$PACKAGE/tags/list") || true
  if [[ $code != 200 ]]; then
    say error "ghcr.io/$PACKAGE isn't reachable from this workflow (HTTP ${code:-none}). A workflow's first push would create it public, like this repository, so the owner creates it privately first: see the steps at the top of $0"
    return 6
  fi
  echo "ghcr.io/$PACKAGE exists, is private, and this workflow can reach it"
}

case ${1:-} in
  pull) shift; pull "$@" ;;
  lock) shift; lock "$@" ;;
  check-private) check_private ;;
  check-publish) check_publish ;;
  *) awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0" >&2; exit 1 ;;
esac
