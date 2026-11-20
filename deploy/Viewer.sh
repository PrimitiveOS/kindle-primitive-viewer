#!/bin/sh
# Name: Viewer
# Author: Primitive AI
# DontUseFBInk
# PrimitiveUSBLease

app=/mnt/us/viewer/bin/kl-viewer-app
[ -x "$app" ] || exit 1
if [ -n "${KL_NATIVE_APP_RUN:-}" ]; then
    cp "$app" "$KL_NATIVE_APP_RUN/run" && chmod 755 "$KL_NATIVE_APP_RUN/run" || exit 1
    cd / || exit 1
    exec "$KL_NATIVE_APP_RUN/run" "$@"
fi

case "$0" in
    /var/tmp/primitive-viewer.*/launch) work=${0%/launch} ;;
    *)
        work=$(mktemp -d /var/tmp/primitive-viewer.XXXXXX) || exit 1
        if ! cp "$0" "$work/launch"; then
            rmdir "$work"
            exit 1
        fi
        cd / || exit 1
        exec /bin/sh "$work/launch" "$@"
        ;;
esac
trap 'rm -rf "$work"' EXIT
cp "$app" "$work/run" && chmod 755 "$work/run" || exit 1
"$work/run" "$@"
