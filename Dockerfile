FROM docker.io/rust:slim-bullseye as builder
RUN apt-get update \
  && apt-get install -y librust-pkg-config-dev libssl-dev \
  && rm -rf /var/lib/apt/lists/*
# TODO: use stable tool chain as soon as let chains are stable
RUN rustup default nightly
WORKDIR /usr/src/harvester
COPY . .
RUN cargo install --path .

FROM docker.io/debian:bullseye-slim
RUN apt-get update \
  && apt-get install -y openssl ca-certificates \
  && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/harvester /usr/local/bin/harvester
WORKDIR /home/harvester
ENTRYPOINT ["harvester"]
