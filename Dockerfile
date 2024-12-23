FROM ubuntu:22.04

USER root

RUN apt-get update && apt-get upgrade -y &&\
    apt-get install -y --no-install-recommends \
        sudo=1.9.9-1ubuntu2.4 \
        ca-certificates=20240203~22.04.1 &&\
    update-ca-certificates &&\
    apt-get clean && rm -rf /var/lib/apt/lists/*

RUN useradd -m student &&\
    usermod -aG sudo student &&\
    echo "student ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/student &&\
    chmod 0440 /etc/sudoers.d/student

USER root

RUN OLD_UID="$(id -u student)" && \
    OLD_GID="$(id -g student)" && \
    NEW_UID=1001 && \
    NEW_GID=1001 && \
    groupmod -g "$NEW_GID" student && \
    usermod -u "$NEW_UID" -g "$NEW_GID" student && \
    find /home -user "$OLD_UID" -execdir chown -h "$NEW_UID" {} + && \
    find /home -group "$OLD_GID" -execdir chgrp -h "$NEW_GID" {} +

RUN apt-get update && apt-get upgrade -y &&\
    apt-get install -y --no-install-recommends \
        vim=2:8.2.3995-1ubuntu2.21 \
        tmux=3.2a-4ubuntu0.2 \
        emacs-nox=1:27.1+1-3ubuntu5.2 \
        curl=7.81.0-1ubuntu1.20 \
        wget=1.21.2-2ubuntu1.1 \
        bzip2=1.0.8-5build1 \
        build-essential=12.9ubuntu3 \
        file=1:5.41-3ubuntu0.1 \
        net-tools=1.60+git20181103.0eebece-1ubuntu5 \
        libssl-dev=3.0.2-0ubuntu1.18 &&\
    apt-get install -y --no-install-recommends \
        gcc-arm-linux-gnueabihf=4:11.2.0-1ubuntu1 \
        g++-arm-linux-gnueabihf=4:11.2.0-1ubuntu1 \
        binutils-arm-linux-gnueabi=2.38-4ubuntu2.6 \
        qemu-user-static=1:6.2+dfsg-2ubuntu6.24 \
        libc6-dbg-armhf-cross=2.35-0ubuntu1cross3 &&\
    apt-get clean && rm -rf /var/lib/apt/lists/*

ENV QEMU_LD_PREFIX=/usr/arm-linux-gnueabihf/
ENV PL_USER student

RUN wget --progress=dot:giga https://static.jyh.sb/source/valgrind-3.22.0.tar.bz2 &&\
    tar -jxf valgrind-3.22.0.tar.bz2
WORKDIR /valgrind-3.22.0

RUN sed -i 's/armv7/arm/g' ./configure &&\
    ./configure --host=arm-linux-gnueabi \
                --prefix=/usr/local \
                CFLAGS=-static \
                CC=/usr/bin/arm-linux-gnueabihf-gcc \
                CPP=/usr/bin/arm-linux-gnueabihf-cpp &&\
    make CFLAGS+="-fPIC" &&\
    make install &&\
    cp /usr/arm-linux-gnueabihf/lib/ld-linux-armhf.so.3 /lib/ &&\
    cp -r /usr/arm-linux-gnueabihf/lib/debug/ /usr/lib/debug &&\
    ln -s /usr/arm-linux-gnueabihf/lib/libc.so.6 /lib/
WORKDIR /
RUN rm -rf valgrind-3.22.0 valgrind-3.22.0.tar.bz2 &&\
    mv /usr/local/libexec/valgrind/memcheck-arm-linux /usr/local/libexec/valgrind/memcheck-arm-linux-wrapper &&\
    echo '#!/bin/bash' > /usr/local/libexec/valgrind/memcheck-arm-linux &&\
    echo 'exec qemu-arm-static /usr/local/libexec/valgrind/memcheck-arm-linux-wrapper "$@"' >> /usr/local/libexec/valgrind/memcheck-arm-linux &&\
    chmod +x /usr/local/libexec/valgrind/memcheck-arm-linux

RUN apt-get update && apt-get upgrade -y && apt-get install -y --no-install-recommends \
        gdb-multiarch=12.1-0ubuntu1~22.04.2 &&\
    mv /usr/bin/gdb /usr/bin/gdb-x86 &&\
    apt-get clean && rm -rf /var/lib/apt/lists/*

COPY gdb /usr/bin/gdb

RUN chmod +x /usr/bin/gdb &&\
    mv /usr/bin/objdump /usr/bin/objdump-x86 &&\
    mv /usr/bin/arm-linux-gnueabihf-objdump /usr/bin/objdump &&\
    mv /usr/bin/gcc /usr/bin/x86-gcc &&\
    mv /usr/bin/arm-linux-gnueabihf-gcc /usr/bin/gcc

COPY hook_execve.c /root/
WORKDIR /root

RUN /bin/bash -o pipefail -c 'QEMU_HASH="$(sha256sum /usr/bin/qemu-arm-static | awk "{print \$1}")" && \
    sed -i "s|PLACEHOLDER_HASH|$QEMU_HASH|g" /root/hook_execve.c' &&\
    /usr/bin/x86-gcc -shared -fPIC -o hook_execve.so hook_execve.c -ldl -lssl -lcrypto &&\
    mv /root/hook_execve.so /usr/lib/hook_execve.so

COPY src /xterm
WORKDIR /xterm

RUN /bin/bash -o pipefail -c "curl -fsSL https://deb.nodesource.com/setup_22.x | bash -" &&\
    apt-get update &&\
    apt-get install -y --no-install-recommends \
        nodejs=22.12.0-1nodesource1 &&\
    npm install -g yarn@1.22.22 &&\
    yarn install --frozen-lockfile &&\
    yarn cache clean &&\
    apt-get clean && rm -rf /var/lib/apt/lists/*

EXPOSE 8080
USER 1001

ENV LD_PRELOAD /usr/lib/hook_execve.so
ENTRYPOINT ["node", "server.js", "-w", "/home/student"]

