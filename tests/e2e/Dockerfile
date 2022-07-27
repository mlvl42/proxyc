FROM debian:bullseye
RUN apt-get update && apt-get install -y microsocks tinyproxy dnsmasq python3
RUN mkdir -p /var/www
RUN echo "OK" > /var/www/index.html
EXPOSE 1080
EXPOSE 1081
EXPOSE 8888
COPY config/start.sh start.sh
COPY config/dnsmasq.conf /etc/dnsmasq.conf
COPY config/tinyproxy.conf /etc/tinyproxy.conf
ENTRYPOINT exec ./start.sh
