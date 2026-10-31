# Session-only directory reporting for Bash and Zsh.
# Author: jaswtao · Date: 2026-10-31
if [ -z "${__orbyte_directory_hook-}" ]; then
    __orbyte_report_directory() {
        local orbyte_status=$?
        printf '\033]9;9;%s\007' "$PWD"
        return "$orbyte_status"
    }
    if [ -n "${BASH_VERSION-}" ]; then
        if (( BASH_VERSINFO[0] > 5 || (BASH_VERSINFO[0] == 5 && BASH_VERSINFO[1] >= 1) )); then
            eval 'PROMPT_COMMAND=(__orbyte_report_directory "${PROMPT_COMMAND[@]}")'
        else
            # Bash before 5.1 only executes the scalar prompt command.
            PROMPT_COMMAND="__orbyte_report_directory"$'\n'"${PROMPT_COMMAND-}"
        fi
        __orbyte_directory_hook=1
    elif [ -n "${ZSH_VERSION-}" ]; then
        eval 'precmd_functions=("${precmd_functions[@]}" __orbyte_report_directory)'
        __orbyte_directory_hook=1
    fi
fi
