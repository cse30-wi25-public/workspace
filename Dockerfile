FROM ubuntu:24.04

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

WORKDIR /root

# x86 tools
RUN apt-get update && apt-get upgrade -y && apt-get install -y --no-install-recommends \
        sudo ca-certificates vim curl wget bzip2 file net-tools build-essential less libssl-dev \
        qemu-user-static

# toolchain & tools symbolic link
RUN curl -L https://static.jyh.sb/source/arm-gnu-toolchain-14.2.rel1-x86_64-arm-none-linux-gnueabihf.tar.xz -O && \
    tar -xvf /root/arm-gnu-toolchain-14.2.rel1-x86_64-arm-none-linux-gnueabihf.tar.xz -C /root/ && \
    mv /root/arm-gnu-toolchain-14.2.rel1-x86_64-arm-none-linux-gnueabihf /usr/arm-gnu-toolchain && \
    rm /root/arm-gnu-toolchain-14.2.rel1-x86_64-arm-none-linux-gnueabihf.tar.xz
ENV QEMU_LD_PREFIX=/usr/arm-gnu-toolchain/arm-none-linux-gnueabihf/libc

# wrapper
RUN mkdir -p /usr/armbin && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-addr2line /usr/armbin/addr2line && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-nm /usr/armbin/nm && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-readelf /usr/armbin/readelf && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-strings /usr/armbin/strings && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-strip /usr/armbin/strip && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-ar /usr/armbin/ar && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-as /usr/armbin/as && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-gcc /usr/armbin/gcc && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-g++ /usr/armbin/g++ && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-cpp /usr/armbin/cpp && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-ld /usr/armbin/ld && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-ranlib /usr/armbin/ranlib && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-gprof /usr/armbin/gprof && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-elfedit /usr/armbin/elfedit && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-objcopy /usr/armbin/objcopy && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-objdump /usr/armbin/objdump && \
    ln -s /usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-size /usr/armbin/size
COPY gdb /usr/armbin/gdb
RUN chmod +x /usr/armbin/gdb

# valgrind
RUN curl -L https://static.jyh.sb/source/valgrind-3.24.0.tar.bz2 -O && \
    tar -jxf valgrind-3.24.0.tar.bz2
WORKDIR /root/valgrind-3.24.0
RUN sed -i 's/armv7/arm/g' ./configure && \
    ./configure --host=arm-none-linux-gnueabihf \
                --prefix=/usr/local \
                CFLAGS=-static \
                CC=/usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-gcc \
                CPP=/usr/arm-gnu-toolchain/bin/arm-none-linux-gnueabihf-cpp && \
    make CFLAGS+="-fPIC" && \
    make install
WORKDIR /root/
RUN rm -rf valgrind-3.24.0 valgrind-3.24.0.tar.bz2 && \
    mv /usr/local/libexec/valgrind/memcheck-arm-linux /usr/local/libexec/valgrind/memcheck-arm-linux-wrapper && \
    echo '#!/bin/bash' > /usr/local/libexec/valgrind/memcheck-arm-linux && \
    echo 'exec qemu-arm-static /usr/local/libexec/valgrind/memcheck-arm-linux-wrapper "$@"' >> /usr/local/libexec/valgrind/memcheck-arm-linux && \
    chmod +x /usr/local/libexec/valgrind/memcheck-arm-linux
ENV VALGRIND_OPTS="--vgdb=no"

# exec hook
COPY hook_execve.c /root/
RUN QEMU_HASH="$(sha256sum /usr/bin/qemu-arm-static | awk "{print \$1}")" && \
    sed -i "s|PLACEHOLDER_HASH|$QEMU_HASH|g" /root/hook_execve.c && \
    /usr/bin/gcc -shared -fPIC -o hook_execve.so hook_execve.c -ldl -lssl -lcrypto && \
    mv /root/hook_execve.so /usr/lib/hook_execve.so
ENV LD_PRELOAD /usr/lib/hook_execve.so

# PL user
RUN useradd -m -s /bin/bash student && \
    usermod -aG sudo student && \
    echo "student ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/student && \
    chmod 0440 /etc/sudoers.d/student
RUN OLD_UID="$(id -u student)" && \
    OLD_GID="$(id -g student)" && \
    NEW_UID=1001 && \
    NEW_GID=1001 && \
    groupmod -g "$NEW_GID" student && \
    usermod -u "$NEW_UID" -g "$NEW_GID" student && \
    find /home -user "$OLD_UID" -execdir chown -h "$NEW_UID" {} + && \
    find /home -group "$OLD_GID" -execdir chgrp -h "$NEW_GID" {} +
ENV PL_USER student

# xterm js
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

ENV PATH="/usr/armbin:$PATH"
USER 1001
ENTRYPOINT ["node", "server.js", "-w", "/home/student"]

