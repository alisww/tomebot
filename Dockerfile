FROM rust:1.52-slim AS build
WORKDIR /usr/src

RUN apt-get update && \
    apt-get dist-upgrade -y && \
    apt-get install -y musl-tools wget build-essential cmake unzip


RUN ln -s /usr/include/x86_64-linux-gnu/asm /usr/include/x86_64-linux-musl/asm \
   && ln -s /usr/include/asm-generic /usr/include/x86_64-linux-musl/asm-generic\
   && ln -s /usr/include/linux /usr/include/x86_64-linux-musl/linux

RUN mkdir /musl


workdir /usr/src/openssl
RUN wget https://github.com/openssl/openssl/archive/OpenSSL_1_1_1f.tar.gz
RUN tar zxvf OpenSSL_1_1_1f.tar.gz
WORKDIR /usr/src/openssl/openssl-OpenSSL_1_1_1f

RUN CC="musl-gcc -fPIE -pie" ./Configure no-shared no-async --prefix=/musl --openssldir=/musl/ssl linux-x86_64
RUN make depend
RUN make -j$(nproc)
RUN make install

ENV PKG_CONFIG_ALLOW_CROSS=1
ENV OPENSSL_STATIC=true
ENV OPENSSL_DIR=/musl

RUN rustup target add x86_64-unknown-linux-musl

RUN USER=root cargo new tomebot
WORKDIR /usr/src/tomebot
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo install --target x86_64-unknown-linux-musl --path .

FROM debian:buster-slim

RUN apt-get update && \
    apt-get dist-upgrade -y && \
    apt-get install wget -y

COPY --from=build /usr/local/cargo/bin/tomebot .
COPY --from=build /usr/local/cargo/bin/eventshook .
USER 1000
CMD ["./tomebot"]
