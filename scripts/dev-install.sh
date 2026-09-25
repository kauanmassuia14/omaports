#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
plugin_id="io.github.kauanmassuia14.portpilot"
config_home="${XDG_CONFIG_HOME:-$HOME/.config}"
plugin_dir="$config_home/omarchy/plugins/$plugin_id"

cargo install --path "$repo_root" --force
mkdir -p "$plugin_dir"
install -m 0644 "$repo_root/manifest.json" "$plugin_dir/manifest.json"
install -m 0644 "$repo_root/BarWidget.qml" "$plugin_dir/BarWidget.qml"
install -m 0644 "$repo_root/Panel.qml" "$plugin_dir/Panel.qml"

printf 'Installed PortPilot CLI and Omarchy plugin sources.\n'
printf 'Rescan with: omarchy-shell shell rescanPlugins\n'
printf 'Enable it with: omarchy plugin enable %s\n' "$plugin_id"
