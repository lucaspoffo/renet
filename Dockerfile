FROM ubuntu:20.04
 
RUN apt-get update && apt-get install -y curl
RUN apt-get install build-essential -y
RUN apt-get install -y iproute2 net-tools iputils-ping

 
RUN mkdir -p /code
WORKDIR /code
 
COPY . /code

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

RUN cargo build --release

