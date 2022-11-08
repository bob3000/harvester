FROM docker.io/rust:1.65-slim-bullseye as builder
RUN apt-get update \
  && apt-get install -y librust-pkg-config-dev libssl-dev \
  && rm -rf /var/lib/apt/lists/*
WORKDIR /usr/src/harvester
COPY . .
RUN cargo install --path .

FROM docker.io/debian:bullseye-slim
RUN apt-get update \
  && apt-get install -y extra-runtime-dependencies \
  && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/harvester /usr/local/bin/harvester
WORKDIR /home/harvester
ENTRYPOINT ["harvester"]
