# Isolated OpenSSH fixture for the ignored Orbyte protocol probe.
# Author: jaswtao · Date: 2026-10-31
FROM alpine:3.20

RUN apk add --no-cache openssh-server openssh-server-pam shadow \
    && adduser -D -s /bin/sh orbyte \
    && mkdir -p /run/sshd /fixtures

COPY entrypoint.sh /entrypoint.sh
RUN chmod 0755 /entrypoint.sh

EXPOSE 22
ENTRYPOINT ["/entrypoint.sh"]
