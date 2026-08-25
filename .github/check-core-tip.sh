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
displaced_packages=()
backup_root=""

cleanup() {
    for link in "${installed_links[@]}"; do
        rm -- "$link"
    done
    for name in "${displaced_packages[@]}"; do
        mv -- "$backup_root/$name" "$package_root/$name"
    done
    if [[ -n "$backup_root" ]]; then
        rmdir "$backup_root"
    fi
    if [[ "$created_package_root" == true ]]; then
        rmdir "$package_root"
    fi
}
trap cleanup EXIT

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

# Core ships some Packages in-tree until they are extracted to the collection. Where a
# collection Package shares a name with one of those, the collection's copy is what is under
# test: its in-tree counterpart is moved aside for the duration of the check and restored on
# exit. Every other in-tree Package is left alone, so the check compiles the union.
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
    if [[ -L "$link" ]]; then
        echo "$link already exists as a symlink; refusing to overwrite" >&2
        exit 1
    fi
    if [[ -e "$link" ]]; then
        if [[ -z "$backup_root" ]]; then
            backup_root=$(mktemp -d)
        fi
        if ! mv -- "$link" "$backup_root/$package_name"; then
            echo "could not move aside the in-tree $package_name to make room for the collection copy" >&2
            exit 1
        fi
        displaced_packages+=("$package_name")
    fi

    ln -s "$package_dir" "$link"
    installed_links+=("$link")
done

cargo +1.93.0 test --manifest-path "$core_root/Cargo.toml" -p lyracore-module --lib
