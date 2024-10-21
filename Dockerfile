FROM rust:1.84.0

EXPOSE 8080

WORKDIR /opt/myapp
COPY . .

RUN cargo build --release
CMD cargo run --release
