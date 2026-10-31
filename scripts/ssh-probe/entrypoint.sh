#!/bin/sh
# Author: jaswtao · Date: 2026-10-31
set -eu

: "${SSH_USER:=orbyte}"
: "${SSH_PASSWORD:?SSH_PASSWORD must be set}"

printf '%s:%s\n' "$SSH_USER" "$SSH_PASSWORD" | chpasswd

if [ -f /fixtures/authorized_keys ]; then
    mkdir -p "/home/$SSH_USER/.ssh"
    cp /fixtures/authorized_keys "/home/$SSH_USER/.ssh/authorized_keys"
    chown -R "$SSH_USER:$SSH_USER" "/home/$SSH_USER/.ssh"
    chmod 0700 "/home/$SSH_USER/.ssh"
    chmod 0600 "/home/$SSH_USER/.ssh/authorized_keys"
fi

ssh-keygen -A

# openssh-server-pam ships a second binary that actually honours UsePAM.
SSHD_BIN="${SSHD_BIN:-/usr/sbin/sshd}"

exec "$SSHD_BIN" -D -e -f /etc/ssh/sshd_config
