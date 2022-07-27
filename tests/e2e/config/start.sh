#!/bin/bash

# launch microsocks
microsocks -p 1080 &
microsocks -u admin -P password -p 1081 &

# launch http proxy
tinyproxy -c /etc/tinyproxy.conf

# start dnsmasq
dnsmasq

# launch http server on localhost
cd /var/www && python3 -m http.server 8000 --bind 127.0.0.1 &

wait -n

exit $?
