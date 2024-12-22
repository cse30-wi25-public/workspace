#!/bin/bash

if [ "$#" -lt 1 ]; then
    echo "Usage: $0 <binary> [<additional-gdb-commands>]"
    exit 1
fi

BINARY="$1"
GDB_COMMANDS="${@:2}"

QEMU_PORT=$(comm -23 <(seq 10000 20000 | sort) <(netstat -tln | awk '{print $4}' | grep -oE '[0-9]+$' | sort | uniq) | shuf -n 1)

/usr/bin/qemu-arm-static -g $QEMU_PORT "$BINARY" &

while ! netstat -tln | grep -q ":$QEMU_PORT"; do
    sleep 0.1
done

gdb-multiarch \
    --ex="file $BINARY" \
    --ex="set arch armv7" \
    --ex="set sysroot /usr/arm-linux-gnueabihf/" \
    --ex="target remote localhost:$QEMU_PORT" \
    --ex="break main" \
    --ex="continue" \
    $GDB_COMMANDS

kill %1
