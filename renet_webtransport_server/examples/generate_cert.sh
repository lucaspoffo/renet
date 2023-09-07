set -e

openssl req -new -newkey rsa:4096 -x509 -sha256 -nodes -days 365 -out localhost.crt -keyout localhost.key
openssl x509 -in localhost.crt -outform der -out localhost.der
openssl rsa -outform der -in localhost.key -out localhost_key.der