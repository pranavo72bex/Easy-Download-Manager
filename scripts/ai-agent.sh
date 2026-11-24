#!/bin/bash
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# The free AI models' turns, for .github/workflows/ai-fix.yml and ai-tests.yml: sourced,
# not run. `ai_try PROMPT_FILE` gives the task to each provider in $PROVIDERS in turn
# (gemini: Gemini CLI on Google AI Studio's free tier, GEMINI_API_KEY; kilo: opencode on
# Kilo Code's gateway, whose free models need no key), each with two tries: a change
# that fails the checks goes back to the same model with what failed. The first change
# that passes is left in the tree and its provider written to $GITHUB_OUTPUT.
#
# A model runs with only a home, a path and its own key: no GitHub token. Its word is
# not taken: `ai_check` holds the change to what CI holds main to, and refuses a change
# to the checks themselves (the workflows, these scripts, xtask, the floors, the test
# runner's settings, Cargo.lock) or one that takes a test away or switches one off.
# A workflow may define `ai_extra_check`, run after the checks pass, which echoes
# `pass` or why not.

AI_ATTEMPTS=${AI_ATTEMPTS:-2}
AI_TIMEOUT=${AI_TIMEOUT:-60m}
GEMINI_MODEL=${GEMINI_MODEL:-}
KILO_MODEL=${KILO_MODEL:-}
# 1 (a feature's task): rows may be added to docs/behaviour-contract.md, never changed or
# taken away.
AI_CONTRACT_ADD=${AI_CONTRACT_ADD:-0}

# Gemini CLI's tools: read, search, edit, and cargo and read-only git in the shell.
# opencode's: the same, Kilo's gateway as an OpenAI-compatible provider, the web denied.
ai_setup() {
    mkdir -p "$HOME/.gemini" .bot
    # Free models come and go: unless one is named, the first that answers now
    # (scripts/ai-ask.py reads each provider's own catalogue).
    if [ -z "$GEMINI_MODEL" ] && [ -n "${GEMINI_API_KEY:-}" ]; then
        GEMINI_MODEL=$(python3 scripts/ai-ask.py pick gemini 2>/dev/null || true)
    fi
    if [ -z "$KILO_MODEL" ]; then
        KILO_MODEL=$(python3 scripts/ai-ask.py pick kilo 2>/dev/null || echo nvidia/nemotron-3-ultra-550b-a55b:free)
    fi
    echo "Models: gemini ${GEMINI_MODEL:-the default of Gemini CLI}, kilo $KILO_MODEL"
    cat >"$HOME/.gemini/settings.json" <<'EOF'
{ "model": { "maxSessionTurns": 120 },
  "tools": { "core": [ "list_directory", "read_file", "glob", "grep_search", "search_file_content",
    "replace", "write_file", "run_shell_command(cargo)", "run_shell_command(git diff)",
    "run_shell_command(git status)", "run_shell_command(git log)" ] } }
EOF
    AI_KILO_CONFIG=$(jq -n --arg m "$KILO_MODEL" '{
      "$schema": "https://opencode.ai/config.json",
      provider: { kilo: { npm: "@ai-sdk/openai-compatible", name: "Kilo Gateway",
        options: { baseURL: "https://api.kilo.ai/api/gateway" },
        models: { ($m): { name: $m } } } },
      permission: { "*": "allow", webfetch: "deny", websearch: "deny", question: "deny",
        bash: { "*": "deny", "cargo *": "allow", "git diff*": "allow",
                "git status*": "allow", "git log*": "allow" } } }')
}

# `ai_run PROVIDER PROMPT`: one turn; 3 when the provider cannot run here (no key).
ai_run() {
    local provider=$1 prompt=$2
    local base=(env -i "HOME=$HOME" "PATH=$PATH" TERM=dumb CI=true CARGO_TERM_COLOR=never)
    case "$provider" in
    gemini)
        [ -n "${GEMINI_API_KEY:-}" ] || return 3
        local model_args=()
        if [ -n "$GEMINI_MODEL" ]; then model_args=(--model "$GEMINI_MODEL"); fi
        # A headless run trusts the checkout only when told to.
        timeout "$AI_TIMEOUT" "${base[@]}" "GEMINI_API_KEY=$GEMINI_API_KEY" GEMINI_CLI_TRUST_WORKSPACE=true \
            gemini --yolo "${model_args[@]}" --prompt "$prompt"
        ;;
    kilo)
        timeout "$AI_TIMEOUT" "${base[@]}" "OPENCODE_CONFIG_CONTENT=$AI_KILO_CONFIG" \
            opencode run --print-logs --auto --model "kilo/$KILO_MODEL" "$prompt"
        ;;
    *) return 3 ;;
    esac
}

# The rules a change is held to without building or running any of it, so the trusted job
# that commits it (which never runs the model's code) can hold it to them again: echoes ok,
# none (nothing changed), forbidden (a guardrail, a contract row, a symlink) or
# removes-a-test. Says why in .bot/check.log.
ai_guard() {
    local changed guarded
    git add -N . 2>/dev/null
    changed=$(git status --porcelain)
    if [ -z "$changed" ]; then echo none; return; fi
    guarded='\.github/|scripts/|tooling/xtask/|\.config/|\.cargo/|\.git|docs/behaviour-baseline/|docs/coverage-floor\.txt|docs/contract-coverage-floor\.txt|docs/ai-notes\.md|docs/ai-skills/|\.claude/|Cargo\.lock|rust-toolchain\.toml|CLAUDE\.md|AGENTS\.md|deny\.toml'
    if [ "$AI_CONTRACT_ADD" != 1 ]; then guarded+='|docs/behaviour-contract\.md'; fi
    guarded=" ($guarded)"
    if grep -qE "$guarded" <<<"$changed"; then
        echo "Changed a file the checks or the rules depend on, which no change may touch:" >>.bot/check.log
        grep -E "$guarded" <<<"$changed" >>.bot/check.log
        echo forbidden
        return
    fi
    # A link could point a later step at anything on the runner.
    if git diff HEAD --summary | grep -E 'mode 120000' >>.bot/check.log; then
        echo "Added a symbolic link (above): no change may." >>.bot/check.log
        echo forbidden
        return
    fi
    # The contract: rows added, none changed or taken away.
    if git diff -U0 HEAD -- docs/behaviour-contract.md | grep -E '^-[^-]' >>.bot/check.log; then
        echo "Changed or removed a row of docs/behaviour-contract.md (lines above): only new rows may be added." >>.bot/check.log
        echo forbidden
        return
    fi
    # A test may be added or changed, never taken away or switched off.
    if git diff -U0 HEAD -- '*.rs' | grep -E '^-[[:space:]]*#\[(tokio::)?test\]|^\+[[:space:]]*#\[ignore' >>.bot/check.log; then
        echo "Removed a #[test] or added #[ignore] (lines above)." >>.bot/check.log
        echo removes-a-test
        return
    fi
    echo ok
}

# The model's code is built and run with no secret in reach: no GitHub token, no model key.
ai_clean() {
    env -u GH_TOKEN -u GITHUB_TOKEN -u GEMINI_API_KEY -u ACTIONS_RUNTIME_TOKEN \
        -u ACTIONS_ID_TOKEN_REQUEST_TOKEN -u ACTIONS_ID_TOKEN_REQUEST_URL "$@"
}

# Echoes one word: pass, none (nothing changed), forbidden, removes-a-test or fail. What
# the checks printed is in .bot/check.log; the coverage summary in .bot/coverage.txt.
ai_check() {
    local guard
    : >.bot/check.log
    guard=$(ai_guard)
    if [ "$guard" != ok ]; then echo "$guard"; return; fi
    ai_clean cargo fmt --all >>.bot/check.log 2>&1
    if ai_clean cargo clippy --workspace --all-targets --locked -- -D warnings >>.bot/check.log 2>&1 &&
        ai_clean cargo xtask layers >>.bot/check.log 2>&1 && ai_clean cargo xtask headers >>.bot/check.log 2>&1 &&
        ai_clean cargo xtask privacy >>.bot/check.log 2>&1 &&
        ai_clean cargo xtask test --profile ci >>.bot/check.log 2>&1 &&
        ai_clean cargo llvm-cov --workspace --locked --summary-only \
            --fail-under-lines "$(cat docs/coverage-floor.txt)" >.bot/coverage.txt 2>>.bot/check.log; then
        if declare -F ai_extra_check >/dev/null; then
            ai_extra_check
        else
            echo pass
        fi
    else
        tail -n 40 .bot/coverage.txt >>.bot/check.log 2>/dev/null
        echo fail
    fi
}

# The change, as a patch the trusted job applies (git apply runs nothing): every file but
# .bot/, binary-safe.
ai_patch() {
    git add -A -- . && git diff --cached --binary HEAD >"$1" && git reset -q
}

# `ai_try PROMPT_FILE`: each provider, from a clean tree, until a change passes.
ai_try() {
    local prompt provider attempt status result report=""
    prompt=$(cat "$1")
    for provider in $PROVIDERS; do
        git reset -q && git checkout -q -- . && git clean -qfd
        rm -f .bot/summary.md
        local ask=$prompt
        for attempt in $(seq 1 "$AI_ATTEMPTS"); do
            echo "::group::$provider, try $attempt"
            ai_run "$provider" "$ask"
            status=$?
            echo "::endgroup::"
            if [ "$status" = 3 ]; then
                report+="- $provider: cannot run here (no key), skipped"$'\n'
                break
            fi
            echo "::group::$provider, try $attempt: checking the change"
            git status --porcelain
            result=$(ai_check)
            tail -n 60 .bot/check.log
            echo "::endgroup::"
            report+="- $provider, try $attempt: agent exit $status, change: $result"$'\n'
            if [ "$result" = pass ]; then
                echo "provider=$provider" >>"$GITHUB_OUTPUT"
                { echo 'report<<REPORT_END'; printf '%s' "$report"; echo 'REPORT_END'; } >>"$GITHUB_OUTPUT"
                return 0
            fi
            # Nothing changed: a second try would see the same. Otherwise the model is
            # told what failed; a change to a file it may not touch is undone first.
            [ "$result" = none ] && break
            if [ "$result" = forbidden ] || [ "$result" = removes-a-test ]; then
                git reset -q && git checkout -q -- . && git clean -qfd
            fi
            ask="$prompt

Your previous change was checked and refused ($result). What the checks printed is in
.bot/check.log; read it, then make a change that passes. The tree still holds your change
unless it was refused for touching a file it may not, or for removing or ignoring a test."
        done
    done
    { echo 'report<<REPORT_END'; printf '%s' "$report"; echo 'REPORT_END'; } >>"$GITHUB_OUTPUT"
    return 1
}
