FROM rust:1.82.0

EXPOSE 8080

WORKDIR /opt/myapp
COPY . .

RUN cargo build --release --color=never
CMD cargo run --release --color=never
