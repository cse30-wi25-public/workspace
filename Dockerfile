FROM ubuntu:22.04

RUN apt-get update &&\
    apt-get install -y sudo vim curl wget bzip2 build-essential file gcc-arm-linux-gnueabihf binutils-arm-linux-gnueabi qemu-user qemu-user-static gcc g++ libc6-dbg-armhf-cross net-tools

ENV QEMU_LD_PREFIX=/usr/arm-linux-gnueabihf/

RUN wget --progress=dot:giga https://sourceware.org/pub/valgrind/valgrind-3.22.0.tar.bz2 &&\
    tar -jxf valgrind-3.22.0.tar.bz2
WORKDIR /valgrind-3.22.0
RUN sed -i 's/armv7/arm/g' ./configure &&\
    ./configure --host=arm-linux-gnueabi \
            --prefix=/usr/local \
            CFLAGS=-static \
            CC=arm-linux-gnueabihf-gcc \
            CPP=arm-linux-gnueabihf-cpp &&\
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
    chmod +x /usr/local/libexec/valgrind/memcheck-arm-linux &&\
    mv /usr/local/bin/valgrind /usr/local/bin/valgrind-arm &&\
    echo '#!/bin/bash' > /usr/local/bin/valgrind &&\
    echo 'exec qemu-arm-static /usr/local/bin/valgrind-arm "$@"' >> /usr/local/bin/valgrind &&\
    chmod +x /usr/local/bin/valgrind

RUN apt-get install -y gdb-multiarch &&\
    mv /usr/bin/gdb /usr/bin/gdb-x86

COPY arm /usr/bin/arm
COPY gdb /usr/bin/gdb

RUN chmod +x /usr/bin/arm /usr/bin/gdb &&\
    mv /usr/bin/objdump /usr/bin/objdump-x86 &&\
    mv /usr/bin/arm-linux-gnueabihf-objdump /usr/bin/objdump &&\
    mv /usr/bin/gcc /usr/bin/gcc-x86 &&\
    mv /usr/bin/arm-linux-gnueabihf-gcc /usr/bin/gcc

RUN useradd -m -s /bin/bash student && \
    usermod -aG sudo student && \
    echo "student ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers

USER student

WORKDIR /home/student

CMD ["/bin/bash"]
