#!/usr/bin/env bash
# Downloads prebuilt executables only. Does not compile, modify credentials, or start services.
set -euo pipefail

install_root=${1:-/home/rustfs_user/rustfs_data/magpie}
zot_version=${ZOT_VERSION:-0.4.21}
repository=xirf/magpie
branch=feature/discord-coding-agent

for tool in gh git sha256sum tar install realpath stat; do
  command -v "$tool" >/dev/null || { echo "Missing required tool: $tool" >&2; exit 1; }
done
case $(uname -m) in
  aarch64|arm64) arch=arm64 ;;
  x86_64|amd64) arch=amd64 ;;
  *) echo "Unsupported CPU architecture" >&2; exit 1 ;;
esac

# Require the destination parent to exist on a mounted non-root filesystem.
parent=$(realpath -e "$(dirname "$install_root")")
if [[ $(stat -c %d "$parent") == $(stat -c %d /) ]]; then
  echo "Refusing to install on the root filesystem. Mount the SD card first." >&2
  exit 1
fi
mkdir -p "$install_root"
install_root=$(realpath -e "$install_root")
if [[ $(stat -c %d "$install_root") == $(stat -c %d /) ]]; then
  echo "Destination resolves to the root filesystem; refusing installation." >&2
  exit 1
fi
if command -v systemctl >/dev/null; then
  for unit in magpie.service magpie-sdcard.service; do
    if systemctl is-active --quiet "$unit"; then
      echo "Stop $unit before replacing runtime binaries." >&2
      exit 1
    fi
  done
fi

mkdir -p "$install_root/bin" "$install_root/data/tmp" "$install_root/data/cache"
staging=$(mktemp -d "$install_root/data/tmp/install.XXXXXX")
trap 'rm -rf -- "$staging"' EXIT

head_sha=$(gh api "repos/$repository/commits/feature%2Fdiscord-coding-agent" --jq '.sha')
run_id=$(gh api "repos/$repository/actions/runs?branch=feature%2Fdiscord-coding-agent&head_sha=$head_sha&status=success&per_page=1" \
  --jq '.workflow_runs[0].id // empty')
if [[ -z $run_id ]]; then
  echo "No successful build exists for the current feature branch. Wait for CI." >&2
  exit 1
fi
gh run download "$run_id" --repo "$repository" --name "magpie-linux-$arch" --dir "$staging"
archive="magpie-linux-$arch.tar.gz"
(cd "$staging" && sha256sum --check "$archive.sha256")
tar -xOf "$staging/$archive" magpie > "$staging/magpie"
tar -xOf "$staging/$archive" magpie-cli > "$staging/magpie-cli"

zot_archive="zot_${zot_version}_linux_${arch}.tar.gz"
gh release download "v$zot_version" --repo patriceckhart/zot \
  --pattern "$zot_archive" --pattern checksums.txt --dir "$staging"
expected=$(awk -v name="$zot_archive" '$2 == name { print $1 }' "$staging/checksums.txt")
actual=$(sha256sum "$staging/$zot_archive")
actual=${actual%% *}
if [[ ${#expected} != 64 || $actual != "$expected" ]]; then
  echo "Zot checksum verification failed" >&2
  exit 1
fi
tar -xOf "$staging/$zot_archive" zot > "$staging/zot"
install -m 0755 "$staging/magpie" "$staging/magpie-cli" "$staging/zot" "$install_root/bin/"
printf 'Installed Magpie build %s and Zot %s into %s/bin\n' "$head_sha" "$zot_version" "$install_root"
printf 'Configure data/config.yml and data/runtime.env, then link the SD-card systemd unit.\n'
