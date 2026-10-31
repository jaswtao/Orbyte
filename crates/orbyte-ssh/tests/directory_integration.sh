#!/bin/sh
# Run with: sh directory_integration.sh bash|zsh /path/to/directory-integration.sh
# Uses real interactive prompts without changing shell configuration files.
# Author: jaswtao · Date: 2026-10-31
set -eu
shell=$1
ORBYTE_DIRECTORY_INTEGRATION=$2
export ORBYTE_DIRECTORY_INTEGRATION
test_root=$(mktemp -d)
export ORBYTE_DIRECTORY_TEST_ROOT=$test_root
mkdir "$test_root/中文 space"
trap 'rmdir "$test_root/中文 space" "$test_root"' EXIT

case "$shell" in
    *bash)
        if "$shell" -c '(( BASH_VERSINFO[0] > 5 || (BASH_VERSINFO[0] == 5 && BASH_VERSINFO[1] >= 1) ))'; then
            modes='scalar array legacy'
        else
            modes='legacy'
        fi
        set -- "$shell" --noprofile --norc -i
        ;;
    *zsh) set -- "$shell" -f -i; modes='zsh' ;;
    *) exit 2 ;;
esac

for mode in $modes; do
    export ORBYTE_DIRECTORY_TEST_MODE=$mode
    output=$("$@" 2>/dev/null <<'COMMANDS'
PS1=''
if [ -n "${BASH_VERSION-}" ]; then
    if [ "$ORBYTE_DIRECTORY_TEST_MODE" = array ]; then
        PROMPT_COMMAND=('printf "__previous:%s\n" "$?"' 'printf "__second\n"')
    else
        PROMPT_COMMAND='printf "__previous:%s\n" "$?"'
    fi
else
    __previous_hook() { printf '__previous:%s\n' "$?"; }
    precmd_functions=(__previous_hook)
fi
if [ "$ORBYTE_DIRECTORY_TEST_MODE" = legacy ]; then
    # Exercise the pre-5.1 branch on modern Bash too, without changing its readonly version array.
    orbyte_legacy_script=$(sed 's/BASH_VERSINFO\[0\]/4/g; s/BASH_VERSINFO\[1\]/4/g' "$ORBYTE_DIRECTORY_INTEGRATION")
    eval "$orbyte_legacy_script"
    eval "$orbyte_legacy_script"
    case "$PROMPT_COMMAND" in
        *__orbyte_report_directory*__orbyte_report_directory*) exit 1 ;;
        *) printf '__legacy_single_hook\n' ;;
    esac
else
    . "$ORBYTE_DIRECTORY_INTEGRATION"
    . "$ORBYTE_DIRECTORY_INTEGRATION"
fi
if [ -n "${BASH_VERSION-}" ]; then printf '__hooks:%s\n' "${#PROMPT_COMMAND[@]}"; else printf '__hooks:%s\n' "${#precmd_functions[@]}"; fi
false
cd "$ORBYTE_DIRECTORY_TEST_ROOT/中文 space"
cd ..
cd - >/dev/null
cd "$ORBYTE_DIRECTORY_TEST_ROOT/missing"
exit 0
COMMANDS
)
    report=$(printf '\033]9;9;%s\007' "$test_root/中文 space")
    printf '%s' "$output" | grep -F "$report" >/dev/null
    report=$(printf '\033]9;9;%s\007' "$test_root")
    printf '%s' "$output" | grep -F "$report" >/dev/null
    printf '%s' "$output" | grep -F '__previous:1' >/dev/null
    if [ "$mode" = array ]; then
        printf '%s' "$output" | grep -F '__hooks:3' >/dev/null
        printf '%s' "$output" | grep -F '__second' >/dev/null
    elif [ "$mode" = legacy ]; then
        printf '%s' "$output" | grep -F '__hooks:1' >/dev/null
        printf '%s' "$output" | grep -F '__legacy_single_hook' >/dev/null
    else
        printf '%s' "$output" | grep -F '__hooks:2' >/dev/null
    fi
    printf '%s %s: directory reports, existing hooks, exit status and idempotence passed\n' "$shell" "$mode"
done
