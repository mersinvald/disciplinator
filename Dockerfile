FROM rust:1.31.1-stretch

RUN cd /opt && git clone https://github.com/mersinvald/disciplinator.git &&\
    cd disciplinator && cargo build --release

WORKDIR /opt/disciplinator

COPY .fitbit_token /opt/disciplinator/
COPY headmaster.toml /opt/disciplinator/

CMD /opt/disciplinator/target/release/headmaster-bin
