# For loongarch64-unknown-linux-gnu / loongarch64-unknown-linux-musl
# This file can be used with cross directly
ARG TARGET=loongarch64-unknown-linux-gnu
ARG CROSS_BASE_IMAGE=ghcr.io/cross-rs/loongarch64-unknown-linux-gnu:main


FROM $CROSS_BASE_IMAGE AS llvm-builder

ARG TARGET
ARG LLVM_VERSION="21.1.8"

RUN apt-get update && apt-get install python3 ninja-build -y --no-install-recommends

RUN git clone https://github.com/llvm/llvm-project.git --branch llvmorg-$LLVM_VERSION --depth=1 /root/llvm

WORKDIR /root/llvm
RUN cmake -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        # Build Inkwell supported targets only
        -DLLVM_TARGETS_TO_BUILD='AArch64;LoongArch;RISCV;X86' \
        -DLLVM_ENABLE_PROJECTS=llvm \
        -DLLVM_ENABLE_RUNTIMES='' \
        -DLLVM_BUILD_TOOLS=ON \
        -DLLVM_BUILD_UTILS=OFF \
        -DCMAKE_TOOLCHAIN_FILE=/opt/toolchain.cmake \
        -DLLVM_HOST_TRIPLE=$TARGET \
        -DCMAKE_INSTALL_PREFIX=/usr/local/llvm \
        -S llvm \
        -B build

RUN cmake --build build
RUN cmake --build build --target install


FROM $CROSS_BASE_IMAGE AS libffi-builder

ARG TARGET
ARG LIBFFI_VERSION="3.5.2"

RUN curl -sL -o /tmp/libffi-$LIBFFI_VERSION.tar.gz https://github.com/libffi/libffi/releases/download/v$LIBFFI_VERSION/libffi-$LIBFFI_VERSION.tar.gz
RUN tar -C /root -xf /tmp/libffi-$LIBFFI_VERSION.tar.gz
WORKDIR /root/libffi-$LIBFFI_VERSION
RUN ./configure --host=$TARGET --build=x86_64-linux-gnu --prefix=/usr/local/libffi
RUN make
RUN make install


FROM $CROSS_BASE_IMAGE

ARG TARGET

COPY --from=llvm-builder /usr/local/llvm /usr/local/llvm
ENV LLVM_SYS_211_PREFIX=/usr/local/llvm

COPY --from=libffi-builder /usr/local/libffi/lib/*  /x-tools/$TARGET/$TARGET/sysroot/usr/local/lib/
COPY --from=libffi-builder /usr/local/libffi/include/*  /x-tools/$TARGET/$TARGET/sysroot/usr/local/include/

