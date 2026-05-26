#!/usr/bin/env bash
# goctl shell-config installer.
#
# Adds a managed block to your shell rc (~/.zshrc, ~/.bashrc, or
# ~/.bash_profile on macOS) that:
#   1. Exports GOCTL_CONFIG pointing at this repo's regions.yaml
#      (or regions.example.yaml as a fallback).
#   2. Defines goctl_load_env() — a function that exports every GGO_*
#      variable from this repo's .env safely (handles special chars).
#   3. Auto-runs goctl_load_env when each new shell starts.
#
# After running this once, every new terminal has GOCTL_CONFIG set and
# all your GGO_* credentials exported — no manual sourcing per session.
#
# Re-run goctl_load_env in any shell after editing .env to pick up
# changes without opening a fresh terminal.
#
# Idempotent: re-running replaces the previous managed block in place.
#
# Usage:
#   ./setup-shell.sh             # install / refresh
#   ./setup-shell.sh --uninstall # remove the managed block

set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")" && pwd)"
MARKER_START="# --- goctl (managed by identity-gbggo-cli setup-shell.sh) ---"
MARKER_END="# --- end goctl ---"

# ── Pick the shell rc file ────────────────────────────────────────────
RC_FILE=""
case "${SHELL:-/bin/zsh}" in
  *zsh)
    RC_FILE="$HOME/.zshrc"
    ;;
  *bash)
    # macOS interactive terminals load ~/.bash_profile, Linux loads ~/.bashrc.
    if [[ -f "$HOME/.bash_profile" ]]; then
      RC_FILE="$HOME/.bash_profile"
    else
      RC_FILE="$HOME/.bashrc"
    fi
    ;;
  *)
    RC_FILE="$HOME/.profile"
    ;;
esac
touch "$RC_FILE"

# ── Strip any existing managed block (idempotent) ─────────────────────
if grep -qF "$MARKER_START" "$RC_FILE"; then
  awk -v s="$MARKER_START" -v e="$MARKER_END" '
    BEGIN { skip=0 }
    $0 == s { skip=1; next }
    skip && $0 == e { skip=0; next }
    !skip { print }
  ' "$RC_FILE" > "$RC_FILE.goctl.tmp"
  mv "$RC_FILE.goctl.tmp" "$RC_FILE"
fi

# ── Uninstall mode ────────────────────────────────────────────────────
if [[ "${1:-}" == "--uninstall" ]]; then
  echo "✓ Removed goctl shell config from $RC_FILE"
  echo "  Open a new terminal (or 'source $RC_FILE') to apply."
  exit 0
fi

# ── Pick regions file (prefer regions.yaml, fall back to example) ─────
REGIONS_FILE=""
if [[ -f "$REPO_DIR/regions.yaml" ]]; then
  REGIONS_FILE="$REPO_DIR/regions.yaml"
elif [[ -f "$REPO_DIR/regions.example.yaml" ]]; then
  REGIONS_FILE="$REPO_DIR/regions.example.yaml"
  echo "ℹ  Using regions.example.yaml (no regions.yaml found in repo)."
  echo "   To customise: cp regions.example.yaml regions.yaml && edit, then re-run ./setup-shell.sh"
else
  echo "✗ ERROR: no regions.yaml or regions.example.yaml in $REPO_DIR" >&2
  exit 1
fi

# ── Locate .env (warn if missing, but still install GOCTL_CONFIG) ─────
ENV_FILE="$REPO_DIR/.env"
if [[ ! -f "$ENV_FILE" ]]; then
  echo "⚠  $ENV_FILE not found."
  echo "   cp .env.example .env, fill in real values, then re-run ./setup-shell.sh"
  echo "   (Continuing anyway — GOCTL_CONFIG will be set; goctl_load_env will error until .env exists.)"
fi

# ── Append the managed block ──────────────────────────────────────────
# Outer-$VAR is expanded at install time. \$VAR is preserved literally
# so the rendered block uses the consumer shell's runtime values.
cat >> "$RC_FILE" <<EOF
$MARKER_START
export GOCTL_CONFIG="$REGIONS_FILE"
goctl_load_env() {
  local f="$ENV_FILE"
  if [[ ! -f "\$f" ]]; then
    echo "goctl_load_env: .env not found at \$f" >&2
    return 1
  fi
  local k v count=0
  while IFS='=' read -r k v; do
    if [[ "\$k" =~ ^[A-Z0-9_]+\$ ]]; then
      export "\$k=\$v"
      count=\$((count + 1))
    fi
  done < "\$f"
  return 0
}
goctl_load_env >/dev/null 2>&1 || true
$MARKER_END
EOF

# ── Summary ───────────────────────────────────────────────────────────
echo "✓ Installed goctl shell config in $RC_FILE"
echo "  GOCTL_CONFIG=$REGIONS_FILE"
echo "  goctl_load_env  → exports GGO_* from $ENV_FILE on every new shell"
echo ""
echo "Activate this terminal now:"
echo "  source \"$RC_FILE\""
echo ""
echo "Or open a fresh terminal — it'll be ready automatically."
echo "After editing .env, run 'goctl_load_env' to reload without reopening."
