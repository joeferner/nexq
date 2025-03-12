FROM debian:12.7

RUN \
    mkdir -p /opt/nexq/bin \ 
    && mkdir -p /opt/nexq/conf \
    && chown -R 1000:1000 /opt/nexq
WORKDIR /opt/nexq
COPY --chown=1000:1000 --chmod=444 ./nexq.yml /opt/nexq/conf/nexq.yml
COPY --chown=1000:1000 --chmod=555 ./target/release/nexq_main /opt/nexq/bin/nexq
USER 1000

CMD ["/opt/nexq/bin/nexq", "--config=/opt/nexq/conf/nexq.yml"]
