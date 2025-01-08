#!/bin/bash

GDB_ARGS=()
while [[ "$1" =~ ^- && $# -gt 0 ]]; do
    GDB_ARGS+=("$1")
    shift
done

if [ "$#" -lt 1 ]; then
    usage="Usage: gdb [<GDB options>...] <binary> [<program args>...] [< <input-file>]]

Description:
    This script allows optional stdin redirection to the target program instead of GDB.

Examples:
    gdb ./prog arg1 arg2             # Run prog with arguments arg1 and arg2 under GDB.
    gdb ./prog arg1 arg2 < input.txt # Redirect input.txt to prog's stdin.

Notes:
    - If '<' is used, input redirection applies to the program being debugged, not GDB itself."

    echo "$usage"
    exit 1
fi

BINARY="$1"
shift

exec 3<&0

QEMU_PORT=$(comm -23 <(seq 10000 20000 | sort) \
    <(netstat -tln | awk '{print $4}' | grep -oE '[0-9]+$' | sort -u) |
    shuf -n 1)

/usr/bin/qemu-arm-static -g "$QEMU_PORT" "$BINARY" "$@" <&3 &

while ! netstat -tln | grep -q ":$QEMU_PORT"; do
    sleep 0.1
done

if TTY_DEVICE=$(tty 2>/dev/null); then
    GDB_TTY="$TTY_DEVICE"
else
    GDB_TTY="/dev/tty"
fi

/usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-gdb \
    "${GDB_ARGS[@]}" \
    --ex="file $BINARY" \
    --ex="target remote localhost:$QEMU_PORT" \
    --ex="break main" \
    --ex="continue" \
    <"$GDB_TTY" >"$GDB_TTY"

kill %1
