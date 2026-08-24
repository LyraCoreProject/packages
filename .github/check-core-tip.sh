#!/usr/bin/env bash
set -euo pipefail

collection_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
core_root=${1:-}

if [[ -z "$core_root" || ! -f "$core_root/module/Cargo.toml" ]]; then
    echo "usage: $0 /path/to/LyraCore" >&2
    exit 2
fi

core_root=$(cd "$core_root" && pwd)
package_root="$core_root/packages"
created_package_root=false
installed_links=()

cleanup() {
    for link in "${installed_links[@]}"; do
        rm -- "$link"
    done
    if [[ "$created_package_root" == true ]]; then
        rmdir "$package_root"
    fi
}
trap cleanup EXIT

if [[ -d "$package_root" ]] && find "$package_root" -mindepth 1 -maxdepth 1 -print -quit | grep -q .; then
    echo "$package_root must be empty for an isolated collection check" >&2
    exit 1
fi

if [[ ! -d "$package_root" ]]; then
    mkdir "$package_root"
    created_package_root=true
fi

mapfile -d '' package_dirs < <(
    find "$collection_root" -mindepth 1 -maxdepth 1 -type d ! -name '.*' -print0 | sort -z
)

if [[ ${#package_dirs[@]} -eq 0 ]]; then
    echo "the Official Package Collection contains no Packages" >&2
    exit 1
fi

for package_dir in "${package_dirs[@]}"; do
    package_name=$(basename "$package_dir")
    if [[ ! -d "$package_dir/src" && ! -d "$package_dir/client" ]]; then
        echo "$package_name has neither src/ nor client/" >&2
        exit 1
    fi
    if [[ -d "$package_dir/src" && ! -f "$package_dir/src/mod.rs" ]]; then
        echo "$package_name has src/ but no src/mod.rs" >&2
        exit 1
    fi

    link="$package_root/$package_name"
    ln -s "$package_dir" "$link"
    installed_links+=("$link")
done

cargo +1.93.0 test --manifest-path "$core_root/Cargo.toml" -p lyracore-module --lib
