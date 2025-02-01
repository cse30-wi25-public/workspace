FROM ubuntu:24.04

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

WORKDIR /

# PL user
RUN useradd -m -s /bin/bash student
RUN OLD_UID="$(id -u student)" && \
    OLD_GID="$(id -g student)" && \
    NEW_UID=1001 && \
    NEW_GID=1001 && \
    groupmod -g "$NEW_GID" student && \
    usermod -u "$NEW_UID" -g "$NEW_GID" student && \
    find /home -user "$OLD_UID" -execdir chown -h "$NEW_UID" {} + && \
    find /home -group "$OLD_GID" -execdir chgrp -h "$NEW_GID" {} +
ENV PL_USER student

# x86 tools
RUN apt-get update && apt-get install -y --no-install-recommends \
        sudo gosu ca-certificates curl wget bzip2 net-tools build-essential libssl-dev \
        vim neovim emacs-nox nano tmux ssh git less file xxd && \
    # helix
    curl -L https://github.com/helix-editor/helix/releases/download/25.01/helix-25.01-x86_64-linux.tar.xz | tar -xJv -C / &&\
    rm -rf /helix-25.01-x86_64-linux/runtime/grammars &&\
    mv /helix-25.01-x86_64-linux/hx /usr/bin &&\
    mv /helix-25.01-x86_64-linux/runtime /usr/bin/runtime &&\
    rm -rf /helix-25.01-x86_64-linux

# timezone
RUN ln -sf /usr/share/zoneinfo/America/Los_Angeles /etc/localtime \
    && echo "America/Los_Angeles" > /etc/timezone \
    && dpkg-reconfigure -f noninteractive tzdata

# set 'vim' command to use the native vim
# set emacs native compile to use x86 gcc
RUN update-alternatives --set vim /usr/bin/vim.basic && \
    echo "(setq native-comp-driver-options '(\"-B/usr/bin/\" \"-fPIC\" \"-O2\"))" >> /etc/emacs/site-start.d/00-native-compile.el

# arm gnu toolchain
RUN curl -L https://github.com/multiarch/qemu-user-static/releases/download/v7.2.0-1/qemu-arm-static -o /usr/bin/qemu-arm-static && \
    chmod +x /usr/bin/qemu-arm-static && \
    curl -L https://static.jyh.sb/source/arm-gnu-toolchain-14.2.rel1-x86_64-arm-none-linux-gnueabihf.tar.xz -O && \
    tar -xvf /arm-gnu-toolchain-14.2.rel1-x86_64-arm-none-linux-gnueabihf.tar.xz -C / && \
    mv /arm-gnu-toolchain-14.2.rel1-x86_64-arm-none-linux-gnueabihf /usr/arm-gnu-toolchain && \
    rm /arm-gnu-toolchain-14.2.rel1-x86_64-arm-none-linux-gnueabihf.tar.xz
ENV QEMU_LD_PREFIX=/usr/arm-gnu-toolchain/arm-none-linux-gnueabihf/libc

# symbolic link
RUN ln -s /usr/arm-gnu-toolchain/bin/* /usr/bin/ &&\
    mkdir -p /usr/armbin && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-addr2line /usr/armbin/addr2line && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-nm /usr/armbin/nm && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-readelf /usr/armbin/readelf && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-strings /usr/armbin/strings && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-strip /usr/armbin/strip && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-ar /usr/armbin/ar && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-as /usr/armbin/as && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-g++ /usr/armbin/g++ && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-cpp /usr/armbin/cpp && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-ld /usr/armbin/ld && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-ranlib /usr/armbin/ranlib && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-gprof /usr/armbin/gprof && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-elfedit /usr/armbin/elfedit && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-objcopy /usr/armbin/objcopy && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-objdump /usr/armbin/objdump && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-size /usr/armbin/size && \
    echo '#!/bin/bash' > /usr/armbin/gcc && \
    echo 'exec /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-gcc -marm "$@"' >> /usr/armbin/gcc && \
    chmod +x /usr/armbin/gcc

# gdb wrapper & man page
RUN apt-get update -y && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y unminimize man-db && \
    yes | /usr/bin/unminimize || [ $? -eq 141 ]
    # mkdir -p /usr/local/man/man1
COPY gdb cse30db /usr/armbin/
RUN chmod +x /usr/armbin/gdb /usr/armbin/cse30db
COPY cse30db.1 /usr/local/man/man1/

# cross compile valgrind
RUN curl -L https://static.jyh.sb/source/valgrind-3.24.0.tar.bz2 -O && \
    tar -jxf valgrind-3.24.0.tar.bz2
WORKDIR /valgrind-3.24.0
RUN sed -i 's/armv7/arm/g' ./configure && \
    ./configure --host=arm-none-linux-gnueabihf \
                --prefix=/usr/local \
                CFLAGS=-static \
                CC=/usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-gcc \
                CPP=/usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-cpp && \
    make CFLAGS+="-fPIC" -j"$(nproc)" && \
    make install
WORKDIR /
RUN rm -rf valgrind-3.24.0 valgrind-3.24.0.tar.bz2 && \
    mv /usr/local/libexec/valgrind/memcheck-arm-linux /usr/local/libexec/valgrind/memcheck-arm-linux-wrapper && \
    echo '#!/bin/bash' > /usr/local/libexec/valgrind/memcheck-arm-linux && \
    echo 'exec qemu-arm-static /usr/local/libexec/valgrind/memcheck-arm-linux-wrapper "$@"' >> /usr/local/libexec/valgrind/memcheck-arm-linux && \
    chmod +x /usr/local/libexec/valgrind/memcheck-arm-linux
ENV VALGRIND_OPTS "--vgdb=no"

# exec hook
COPY hook_execve.c check_arch_arm.c /
RUN QEMU_HASH="$(sha256sum /usr/bin/qemu-arm-static | awk "{print \$1}")" && \
    sed -i "s|PLACEHOLDER_HASH|$QEMU_HASH|g" /hook_execve.c && \
    /usr/bin/gcc -shared -fPIC -o hook_execve.so hook_execve.c -ldl -lssl -lcrypto && \
    /usr/bin/gcc -o check_arch_arm check_arch_arm.c && \
    mv /hook_execve.so /usr/lib/hook_execve.so && \
    mv /check_arch_arm /usr/bin/check_arch_arm && \
    rm hook_execve.c check_arch_arm.c
ENV LD_PRELOAD /usr/lib/hook_execve.so

# xterm js
COPY src /xterm
WORKDIR /xterm
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - &&\
    apt-get update && \
    apt-get install -y --no-install-recommends \
        nodejs=22.12.0-1nodesource1 && \
    npm install -g yarn@1.22.22 && \
    yarn install --frozen-lockfile && \
    yarn cache clean && \
    npm uninstall -g yarn && \
    rm -rf /root/.cache/yarn && \
    apt-get clean && rm -rf /var/lib/apt/lists/*
EXPOSE 8080

# gosu helper
COPY --chmod=0755 --chown=root:root container-entry /usr/bin/
USER root
RUN mkdir -p /run /var/run && \
    touch /run/fixuid.ran /var/run/fixuid.ran

ENV PATH "/usr/armbin:$PATH"
USER student
ENTRYPOINT ["/usr/bin/container-entry"]
