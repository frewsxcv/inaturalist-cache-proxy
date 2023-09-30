FROM rust:1.72.1

EXPOSE 8080

WORKDIR /opt/myapp
COPY . .

RUN cargo build --release
CMD cargo run --release
