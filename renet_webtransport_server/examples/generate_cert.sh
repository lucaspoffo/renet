set -e

# installing https://github.com/FiloSottile/mkcert is recommended, because it will install the root CA in the system trust store and also brwosers if supported
mkcert -install
mkcert localhost 127.0.0.1 ::1
openssl x509 -in localhost+2.pem -outform der -out localhost.der
openssl rsa -outform der -in localhost+2-key.pem -out localhost_key.der

mkcert -CAROOT

read rm 
#"Press enter to continue"