FROM rust:1.31.1-stretch

RUN cd /opt && git clone https://github.com/mersinvald/disciplinator.git &&\
    cd disciplinator && cargo build --release &&\
    mkdir /etc/disciplinator

WORKDIR /opt/disciplinator

COPY fitbit_token /etc/disciplinator/
COPY headmaster.toml /etc/disciplinator/

CMD /opt/disciplinator/target/release/headmaster-bin -t /etc/disciplinator/fitbit_token -c /etc/disciplinator/headmaster.toml
